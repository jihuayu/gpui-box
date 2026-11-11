# Existing-tree structural transaction evidence

This freezes [atomic topology and portal moves](../../RETAINED_STRUCTURE_TRANSACTIONS.md)
on base `23f5034`. All 73 hardware source hashes match the final working tree.
Adapter: GTX 1650 Ti / Vulkan 571.96. Validation passes 75 retained-text/WGPU
core tests, 67 retained-only core tests and 11 gallery/font tests including
ordinary/hardware portal clicks. Combined and canvas-only Clippy pass with
`-D warnings`. All 148 hardware pixel cases pass.

Core scenarios cover valid tree rotation with persistent Taffy IDs, fresh-tree
geometry equality, old packet/command reuse, final-equal output, duplicate or
missing parents, root attachment, disconnected cycles, foreign handles,
measured-leaf rejection, revision exhaustion and a failed combined logical
permutation/portal cycle that leaves sources/bindings and published output intact.
Reorder-only key reconciliation now changes two hosts in one revision.

Three new hardware cases combine logical reorder with portal exchange, reject
a cycle without GPU submission, and evacuate surviving portals before retiring
their former host scope. Their retained color binding and inherited translation
still update afterwards. All images match original full GPUI rendering. The
gallery actually clicks the visible portal button and verifies changed pixels
against fresh full rendering while retaining command identity and scope owner.

## CPU structural commit comparison

Three rotating rounds use ten warmups and one hundred measured samples per mode
per round (300 measured samples per mode). Each sample reverses/restores the
same existing children in every host. Transaction construction/list copies and
commit are timed; layout/extraction/GPU are excluded. Final node order, zero
command recording and revision counts are asserted. Sequential host updates
produce `hosts` revisions; a single existing property transaction and the new
general final-topology transaction each produce one revision. The general path
also validates whole-tree parentage/reachability; reorder-only input cannot
express arbitrary reparenting through the old property transaction.

| Hosts × children each | Sequential median / P95 (µs) | One property transaction median / P95 (µs) | Final topology median / P95 (µs) |
| --- | --- | --- | --- |
| 2 × 8 | 2.2 / 3.2 | 1.7 / 3.1 | 1.3 / 2.0 |
| 32 × 32 | 79.7 / 134.7 | 72.7 / 124.8 | 54.1 / 101.5 |

These workloads favor the dedicated structure-only path; they do not establish
performance for sparse edits in large trees. Validation scans all arena slots
and live edges. Raw samples/rounds are in `structure-benchmark.json`; other
regenerated JSON benchmarks are controls with no improvement attributed here.
Atomic creation/removal/property/lifecycle batches remain unimplemented, along
with event/focus/semantics integration and other active RFC acceptance work.
