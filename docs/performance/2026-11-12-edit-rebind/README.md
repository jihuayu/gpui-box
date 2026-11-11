# Atomic endpoint replacement follow-up

This freezes the endpoint lifecycle extension of
[unified edits](../../RETAINED_EDIT_TRANSACTIONS.md) on base `e8c3965`.
All 79 hardware runner source hashes match final source files. The GTX 1650 Ti
Vulkan run passes 153 pixel cases. Core retained-text/WGPU passes 92 tests,
gallery/font passes 12 including real GPU clicks, and integrated Clippy passes
with `-D warnings`. Commands are recorded in `checks.json`.

New core cases stage source changes, detach the old subscription and attach a
different source to the same property. Invalid render input preserves old
sources/subscriptions/stats; successful replacement dispatches only the final
writer. The old source subsequently causes zero sink writes/frame requests,
and the new source drives the endpoint. Explicit detach permits a manual
property write. Provisional bind/unbind cancels synchronization; duplicate
unbind rejects; same-value replacement retains revision and shared frame items.

The added hardware `edit-rebind-final-writer` case updates both sources in one
event, replaces the portal's color binding, checks the independently retained
second output still follows the previous derived source, verifies one revision
and compares actual pixels with fresh full rendering. Later mixed removal
retires both surviving endpoint lifetimes and keeps old snapshots valid.

Full suite benchmark controls were regenerated, with raw samples in JSON.
The unchanged structure replacement protocol still shows draft overhead:

| Replaced nodes beside 1,024 stable nodes | Sequential commit + frame median (µs) | Draft commit + frame median (µs) |
| --- | --- | --- |
| 1 | 737.9 | 1016.1 |
| 32 | 842.1 | 1138.2 |
| 256 | 1722.5 | 2991.9 |

These measure structure replacement, not endpoint-rebinding throughput.
Host-load differences are not evidence of a rebind performance regression or
improvement. The prior [construction-stage evidence](../2026-11-12-edit/README.md)
remains historical. No native-surface/cadence/latency/energy claim is made, and
scope/source/effect creation/disposal inside the unified draft remains open.
