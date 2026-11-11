# Reuse paint commands during solid material patches

The P1 path now retains the exact local command allocation when only a supported
solid fill color changes. `PaintItem::fill_color` owns the current material;
`resolved_quad` combines it with the reusable geometry/decoration recording.
The command's original seed color is no longer authoritative for consumers.
CPU window paint, original full GPUI reference conversion, persistent WGPU
instances, ink bounds and damage all consume the same resolved material.

Backends declaring `solid_color_patch` preserve commands. A fallback consumer
still takes paint-only and records a new canonical command. Decoration changes
also record a new command. Published packets retain their own material values,
so a later live change cannot alter old frames. Delta validation compares
material as well as bounds/commands/clip, preventing a stale material delta from
being accepted merely because its shared command matches.

This closes the 0-command-recording part of the currently supported P1 quad
contract. It does not remove the full snapshot extraction/validation traversal.
The complete update is still not an O(1) path.

Verification passes 39 retained-WGPU core tests and both actual click tests,
including the hardware case. Alpha removal/restoration contributes old/new
damage while retaining command identity. The 66 hardware pixel cases pass;
single solid/decorated color changes report zero layout and command recordings
and upload 160 bytes. The 10,000-signal change also reports zero recordings,
with 1,600,000 bytes in one or two adjacent ranges. Source equality/restore
continues to schedule no GPU work. Clippy checks and desktop build pass.

[Frozen evidence](performance/2026-11-11-material/README.md) includes validation,
device/source identity and representative material images. Existing damage/cache
control timings fluctuate and are not a dedicated material-path speedup study.
No new latency or throughput improvement is inferred from the structural counts.
The [complete-runtime goal](RETAINED_IMPLEMENTATION_PROGRESS.md) remains active.
