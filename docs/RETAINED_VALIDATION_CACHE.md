# Immutable-page GPU packet validation cache

The WGPU consumer now retains one last successfully validated paint snapshot,
its node-to-index map and property-map Arc. It reuses item validation only when
the tree, scale, item count and page layout match and each changed page preserves
the exact node identity at every position. Other pages must have the exact same
Arc as the cached snapshot. The cache itself owns these Arcs, so in-place caller
mutation through `Arc::make_mut` creates another page and cannot bypass checks.
Node reorder/replacement or scale/tree changes rebuild validation and the index.

All changed-page items recheck geometry, scale overflow, quad decoration and text
pairing/scale. Update payloads are checked against the current snapshot through
the persistent index, including property ancestry. Headers, duplicate updates,
removals and explicit order remain checked on every call. A changed effect-map
root rechecks effect definitions, cycles and all item effect references; otherwise
only changed-page references need checking. The [persistent-property stage](RETAINED_PERSISTENT_PROPERTIES.md)
checks the actual cache-owned immutable effect root independently of the outer
property Arc, so translation/clip/scroll changes can retain that validation. A failed check does not advance the
cache. Backend failures/reset retain the existing renderer recovery contract;
reset creates an empty validation cache. Stale-frame rejection remains separate.

`FrameSubmission.validation` reports full scan, checked/reused pages, validated
items and effect-reference visits. A local color patch in a 10,001-item packet
checks 64 items and reuses 156 pages. Directory comparison still reads N/64 Arc
pointers. `set_validation_cache_enabled(false)` forces full packet validation on
the same consumer for a paired performance control; it does not change rendering.
The independent reference renderer always runs complete validation.

Two new core tests cover page mutation, malformed geometry, duplicate identity,
invalid property/delta references, effect cycles/removal, scaling, generation
reuse, reorder/new trees and recovery after rejected packets. Combined core tests
pass 52 cases; retained-only passes 47. Ten shared example tests, the desktop
WGPU build and combined/feature-specific Clippy `-D warnings` pass. Hardware
validation passes 124 pixel scenarios, adding rejection/recovery, order fallback
and reset/full validation. Existing resource and in-flight checks remain green.

[Frozen evidence](performance/2026-11-11-validation-cache/README.md) includes
53 verified source hashes and same-consumer cached/full-validation timing.
At 10,001 items, CPU submit median is 371.8 µs with page reuse and 2,197.1 µs with
forced full validation; both upload 160 bytes and match original full GPUI pixels.
These are scoped wall-clock submission timings, including remaining CPU/backend
work; they exclude UI publication and native presentation. They are not GPU-only
timestamps or sustained FPS. All property references may still be visited on
effect-map changes, while composition, resource collection and backend guards
retain scene-wide work. The cache retains the most recent validated snapshot
until replacement/reset/drop, independently of completion leases; it does not
retain an unbounded frame history or provide physical memory budgeting.
