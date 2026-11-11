# Rounded fills, inside borders and route reasons

The retained command format now supports a solid fill with uniform circular
corner radius and an inside solid border. `QuadDecoration` is an owned typed
property, validated before a transaction publishes. Radius and border width
must be finite and nonnegative; both are resolved against half the shortest
allocated edge. Explicit units are logical pixels, scaled once for GPU input.

CPU canvas paint and retained WGPU conversion both reuse the original GPUI
quad shader. The persistent arena still owns one instance for this command,
so a color-only change of an already decorated quad uploads 160 bytes. A
decoration change takes the paint-only route and rerecords that node's command,
without Taffy layout. A combined color/decoration change is reported as paint
work, rather than claiming the invariants of a color-only fast path.

Transparent fills with visible borders contribute ink/damage. Damage remains a
conservative clipped border-box bound; the border draws inside that box. Hit
testing deliberately uses the same rectangular border-box policy as the
existing UI nodes, including rounded corner areas. This is not a rounded clip
for descendants or a pixel-alpha hit test.

`Commit::decisions` adds an explicit reason for every existing route: supported
stable solid-quad color, unavailable backend color patches, primitive paint,
layout inputs, child order or shared properties. `routes` remains available.
This is the reason-record foundation for the currently supported command types;
general material/resource/semantics capability routing remains in the
[full acceptance table](RETAINED_IMPLEMENTATION_PROGRESS.md).

Validation passes 38 retained-WGPU core tests and both normal-click cases,
including an actual border toggle through signal, frame and GPU pixels. The
GPU suite passes 66 scenarios on GTX 1650 Ti/Vulkan. Five new cases compare
rounded/bordered output, color patch, transparent border, clamped dimensions
and decoration removal against original full GPUI rendering at 1.25 scale.
Independent pixel assertions check preserved corner background and differing
border/fill pixels. The latter cases use preserved-target Damage rendering.
Invalid command data is also rejected by packet validation. Core/gallery/runner
Clippy checks pass with warnings denied, and the desktop example builds.

[Frozen evidence](performance/2026-11-11-primitives/README.md) records the measured
sources and pixels. No new throughput claim is made. Images, glyphs, paths,
shadows, nonuniform decoration and resource epochs remain pending. The full
goal is active; rectangle-family support does not complete primitive coverage.
