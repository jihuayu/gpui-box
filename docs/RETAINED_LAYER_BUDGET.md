# Layer allocation budgets across frames

[Bounded textures](RETAINED_LAYER_BOUNDS.md) reduce each working set, but a live
working-set guard alone does not bound overlapping allocations held by earlier
GPU submissions. The retained renderer now budgets unique isolated-texture
payload across the cache and all in-flight leases within one device epoch.
The [complete runtime goal](RETAINED_IMPLEMENTATION_PROGRESS.md) remains active.

## Reservation and submission contract

Each actual layer texture allocation owns a shared reservation. Every cache
entry and submission lease references the same reservation; sharing a texture
across several frames counts once. Replaced/removed entries relinquish cache
ownership. Only the last reference releases its byte/count reservation. Explicit
GPU resources precede the reservation in the owning structs/tuples, so their
handles drop before the final accounting release. Queue completion and owner
collection determine when in-flight references leave.

Submission first validates desired live extents against the configured limit.
Malformed or individually oversized packets fail before cache pruning, resource
writes and serial advancement. It then polls/collects completed submissions and
applies the existing frames-in-flight bound. Dead/replaced cache entries leave
before allocation admission. If current reserved bytes plus all required new
allocations exceed the cap, the owner waits for its oldest submission and
collects completion, repeating only as necessary. Resource allocation starts
after admission; the allocator independently checks the limit again.

This preserves isolation and draw order under transient memory pressure, at the
cost of queue overlap. It does not flatten opacity, discard effects, evict a
current visible working set or provide a fallback for an individually oversized
scene. Unused cache entries already leave each frame; dormant-cache admission,
LRU, reusable scratch surfaces and measured adaptive policy remain pending.

## Public controls and counters

`WgpuFrameRenderer::set_layer_budget_bytes` and the matching backend API accept a
positive byte cap, defaulting to 256 MiB. Reconfiguration finishes pending work,
collects old leases, clears cached layers, resets the accounting peak epoch and
forces a complete resynchronization/redraw. Setting the same limit preserves the
cache. Zero rejects before mutation. Renderer reset retains the requested cap
and starts counters from zero on the replacement device.

`layer_memory()` observes counters without polling or collecting. Its
`LayerMemoryStats` contains the cap, live cached bytes, resident reserved bytes,
retired bytes, peak resident bytes and unique resident allocation count. Here
resident means explicit texture payload reservations, not operating-system or
driver telemetry. `QuadSubmission::layer_memory` snapshots those values after
submission and `layer_budget_waits` counts waits caused by allocation pressure,
separately from the existing queue-length waits. An idle completion wake followed
by owner `poll` can retire old allocations without producing another frame.

The budget counts RGBA texel payload (including 1×1 empty placeholders). Driver
alignment/pooling, view/buffer overhead, main targets, images/glyphs, arenas,
other renderers and old device epochs are outside this cap. Device reset/failure
may leave internal driver resources retiring beyond owner lifetimes; a global
physical-memory/epoch budget and forced device-loss qualification remain open.

## Verification and performance

The shared reservation tests cover deduplicated multi-frame references, rejection
without mutation, final-reference release, peak history, arithmetic overflow and
leases outliving the budget owner. Hardware checks cover stable cached layers,
invalid reconfiguration, cap persistence after reset, explicit completion wake
and idle retirement, actual pressure waits, and oversized-packet rejection with
an unchanged cache and valid retry. Existing mixed primitive/pixel cases remain
part of the final regression run.

Completed-work controls compare the same renderer and workload under the default
cap and a one-layer cap. The control explicitly disables [origin texture reuse](RETAINED_LAYER_REUSE.md),
so alternating physical origins still require replacement allocations and
exercise budget pressure. Default origin reuse now avoids that replacement when
extents remain stable. Low-overdraw and 128-overlap cases expose the memory/queue-overlap
tradeoff. Both use full root redraw, three frames in flight, rotating mode order,
six warmups and twenty timed frames per round with final GPU completion. Texture
readback/reference checks happen outside timing. [Frozen evidence](performance/2026-11-12-layer-budget/README.md) records 213
hardware scenarios (211 pixel comparisons), 116 core and 12 gallery/font/click
tests, Clippy and the desktop WGPU build. Three repeated controls reduce peak
reserved texture payload threefold (12.41 → 4.14 MiB) with completed-work costs
of about 72.4% for light overdraw and 4.7% for heavy overdraw. All four selected
150-source manifests match the final source; no compiler was observed during
the executions.
