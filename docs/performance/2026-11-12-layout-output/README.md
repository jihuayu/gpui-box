# Actual layout outputs to selective Paint Store publication

This follows [persistent property maps](../2026-11-12-property-maps/README.md)
on base `e3a8b7b`. The [implementation](../../RETAINED_LAYOUT_PUBLICATION.md)
observes real unrounded Taffy writes, compares final layouts with published
baselines and extracts only changed items/moved subtrees when visibility,
identity, order and viewport remain stable. Layout algorithms and caches are
unchanged. The original full extraction remains a forced control.

## Correctness and provenance

Final checks pass 113 combined-feature retained core tests, 12 gallery/font
tests including real hardware clicks, 169 Taffy default-feature library tests
and integrated Clippy `-D warnings`. Retained-only passes 104 tests and the
desktop WGPU executable builds successfully. Logs are included. The distribution's
actual Taffy feature combination is covered by core/example tests; running the
upstream library test module with no default features hits its existing
ungated `CompactLength::calc` test. No upstream test was removed or weakened.

`validation.json` records 171 passing pixel scenarios, including nine additions
for size changes, sibling movement, wrap/scroll/clamp, inherited properties,
mixed decoded-image resources, visibility fallback and actual text wrapping.
Ordinary measurement/paragraph click tests assert selective publication and
compare real GPU pixels. References use original full GPUI for plain quads and
a fresh uncached retained consumer for isolated effects/text/images.

`metadata.json` contains 146 final source hashes, all verified before freezing,
including every vendored Taffy source file. `vendor/taffy/UPSTREAM.json` preserves
65 original package file hashes, the registry checksum and exact Git revision.
Only `src/tree/taffy_tree.rs` differs: the optional generic observation hook and
delegation from the original API. The MIT license was fetched from the exact
revision and its source/digest recorded. `verification.json` records scope and
counts. Complete source/layout algorithm conformance is not inferred from a
pixel check alone.

## Same-tree CPU comparison

`cpu/run-1.json` through `run-3.json` come from one release executable built
with `retained-runtime`. Each run rotates selective/full order across three
rounds, with six warmups and twenty measured events per mode per round. Pooled
results contain 180 samples per mode/case. `cpu/execution.json` records binary
SHA256, start/end times, exits and zero observed compilation processes sampled
approximately every 500 ms. This does not prove an otherwise idle host. Raw
samples, per-run/per-round medians and total commit+frame times remain available;
no outliers were discarded.

The fixture has fixed 80×80 groups, each with 100 leaves, under wrapping Flex.
It changes the last leaf's explicit width, group height, first leaf's width or
last leaf's owned intrinsic width. Both modes use the exact same persistent
tree, transactions, Taffy computation and observer. Complete final paint items,
properties, damage and GPU delta identities are compared outside timing. An old
snapshot remains owned. Publication includes Taffy recomputation, observer
comparison/deduplication, extraction and producer snapshot retirement. Initial
tree/layout, transaction construction, renderer validation/planning, GPU draws
and native presentation are excluded. These are not measurements against an
old executable and do not quantify the hook's overhead versus unpatched Taffy.

| Render nodes | Change | Selectively extracted | Selective layout+publication median / P95 (µs) | Full extraction median / P95 (µs) |
| --- | --- | --- | --- | --- |
| 1,011 | last leaf size | 1 | 29.3 / 38.4 | 872.5 / 1266.9 |
| 1,011 | parent size | 1 | 33.7 / 75.9 | 849.9 / 1078.8 |
| 1,011 | first leaf, dependent movement | 100 | 95.5 / 159.1 | 903.2 / 1231.6 |
| 1,011 | last leaf intrinsic | 1 | 29.3 / 30.2 | 842.9 / 1256.0 |
| 10,101 | last leaf size | 1 | 45.9 / 56.6 | 10891.1 / 12751.7 |
| 10,101 | parent size | 4,041 | 3337.9 / 4268.7 | 11207.3 / 13148.0 |
| 10,101 | first leaf, dependent movement | 100 | 119.8 / 250.4 | 10982.3 / 14525.6 |
| 10,101 | last leaf intrinsic | 1 | 46.2 / 86.7 | 11755.5 / 14924.3 |

At 10,101 nodes the last-leaf case records 201 layout writes, two changed writes
and two net changed layout identities, but extracts one item: root overflow
changes do not affect its geometry or clamped scroll. In the parent-height case,
Flex actually moves later group rows: 42 changed relative layouts expand to
4,041 extracted items because cached descendants inherit new world positions.
That case intentionally demonstrates that dependency movement still scales
with affected content. At 1,011 nodes all groups fit one row, so changing a
group's height affects only its own paint item.

These results support reduced CPU layout-to-paint publication work, not an
application FPS, GPU speedup or input-latency claim. Remaining costs include
Taffy's ancestor/sibling layout work and repeated/equal writes, ordered changed-ID
tracking, N/64 page-directory copies, affected subtree extraction and consumer
resource/order/lifetime scans. Rounded-layout conformance, memory growth,
sustained cadence, energy, native presentation and other platforms remain open.
Root hardware-run benchmark controls are preserved for context; unrelated GPU
differences are not attributed to this stage. `preliminary/` retains the first
CPU run and initial integrated data rather than replacing early observations.

## Reproduction

```powershell
mise exec -- cargo test --locked --offline -p gpui-box -p gpui-box-example-hello --features gpui-box/retained-text,gpui-box/retained-wgpu,gpui-box-example-hello/retained-wgpu -- --include-ignored
mise exec -- cargo test --manifest-path vendor/taffy/Cargo.toml --offline --lib
mise exec -- cargo clippy --locked --offline -p gpui-box -p gpui-box-example-hello -p gpui-box-rendering-perf --features gpui-box-example-hello/retained-wgpu,gpui-box-rendering-perf/retained-gpu --all-targets -- -D warnings
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/layout-output-check
mise exec -- cargo build --release --locked --offline -p gpui-box-rendering-perf --features retained-runtime --bin layout-publication-perf
# After all compilation has exited:
./target/release/layout-publication-perf.exe target/layout-publication.json
```
