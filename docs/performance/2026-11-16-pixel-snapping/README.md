# Device-pixel projection validation — 2026-11-16

Windows x64 / Rust 1.98.1 / locked offline. Contract and tradeoffs:
[RETAINED_PIXEL_SNAPPING.md](../../RETAINED_PIXEL_SNAPPING.md).
No Taffy source/GPU record format changed. No timing benchmark or sustained-frame
qualification was performed in this stage.

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib pixel_snap -- --include-ignored --nocapture
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu hardware_pixel_snap -- --include-ignored --nocapture
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
```

Results: retained **305 passed**; text/WGPU **338 passed, 7 ignored**;
hello **22 passed, 2 ignored**; Clippy `-D warnings` passed. The two new default
ignored hardware cases were separately executed and passed. The retained-only
SpatialUpdate dead-field warning and Windows macro linker informational warnings
remain. Clippy initially requested a named projected-ancestor record instead of
a complex tuple; the refactor and rerun passed.

Seven new ordinary framework regressions cover:

1. Three fractional Flex grow items sharing width 100 at scales 1/1.25/1.5/2.
   Projected cumulative edges share exact tested endpoints, all physical columns
   are covered, hits use the projected bounds, LayoutResult is unchanged, policy
   change uses zero layout/command recording, and None restores original bounds.
2. Atomic source plus policy staging followed by final composed overflow fails
   without publishing source/policy/stats/frame. Equal policy is a no-op;
   zero/negative/NaN/infinite/noninvertible scale rejects. CPU scale mismatch
   rejects and the original snapshot keeps its original logical width.
3. Fractional translated/scrolled/clipped local frames at scale 1.5 match a fresh
   ordinary forced-full tree's projected CPU pixels, with old-frame ownership.
4. A mixed policy/color edit increments revision once; exhaustion rejects both
   mixed edits and the direct setter without changing owner properties/policy.
5. A 10.3-wide ContainerInline host is displayed at 10.5 with scale 2. The 10.4
   breakpoint does not trigger, and an inline ConstraintBuilder still reads 10.3.
6. Nested effective clips, isolated group opacity, image resources, OOF, hide/show,
   reparent and scale/None toggles retain correct geometry/property identities
   and every CPU pixel against fresh full references. Old frames reproduce pixels
   after the mutable owner is dropped.
7. Fractional viewport and root own clip stay distinct. A subsequent child color
   edit keeps damage confined to its visible bounds; it does not falsely mark
   the full viewport changed. Local root clip-property identity is preserved.

Actual bundled Noto Sans CJK SC shaping/rasterization searches a real fractional
width where rounding changes line count. Device projection keeps the exact same
Arc glyph chunk, logical layout and rasterized-glyph count. An independent
ordinary tree retains the logical width and moves the transparent text box origin
to the expected grid; every CPU pixel matches, with visible cyan glyph coverage
and old-frame output after UI/font/raster owners are dropped. The separate
explicit hardware variant compares every GPU pixel to that independent reference.

Explicit hardware: **NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96**.
The framework hardware case checks a logical 10.3×10.2 red box at scale 2 against
an independent ordinary 10.5×10.0 box, exact CPU pixels with the same opaque
black background, and edge hit testing. It rejects mismatched local/resolved
submissions before a valid retry, including an idle retry. It then compares all
pixels of six actual local submissions at scales 1.25/1.5/2/2/2/1.25, including
None, nested clips/opacity/image/OOF, hide/show and reparent, to fresh full trees.
Alternating local and resolved consumers preserves output; idle skips submit.

During development, an exact f32 local-clip round-trip assertion failed at scale
1.5 under negative translated/scroll coordinates. Reusing inherited equal clips
and canonicalizing through the consumer's actual f32 operations resolved it;
arbitrary exact device-grid representation remains outside the contract. The
first integer CPU/GPU comparison also failed because CPU transparent black and
GPU opaque black backgrounds differed; matching the explicit opaque background
made the independent comparison valid. No renderer correction was inferred from
that test-fixture mismatch.
Final review also found that folding a root own clip into root_clip and comparing
projected to logical viewport could force full damage on every update. Keeping
the two clips distinct and comparing projected viewports fixed that issue, with
the seventh regression and root-clip coverage in the explicit GPU fixture.

These are correctness checks on this adapter, not modern desktop heavy-effects,
visible presentation, continuous 60/120Hz, CPU smooth-delivery or tail-latency
evidence. Full dirty extraction/projection and loss of ancestor output sharing
are explicit opt-in costs. Full conformance and the
[remaining acceptance table](../../RETAINED_IMPLEMENTATION_PROGRESS.md) stay open.
