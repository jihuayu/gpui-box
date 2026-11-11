# Persistent property maps: correctness and CPU publication

The [implementation](../../RETAINED_PERSISTENT_PROPERTIES.md) replaces the four
whole-cloned `BTreeMap` property snapshots with immutable shared AVL roots and
path copying. Selective extraction sets/removes current records directly,
retaining equal values and independent property-kind roots. Full extraction
uses a bulk builder. Base commit: `6d5c8d9`.

Final validation passes 108 combined-feature core tests, 12 gallery/font tests
including an ordinary hardware click, and integrated Clippy `-D warnings`.
`validation.json` contains 162 passing hardware pixel cases on Windows,
GTX 1650 Ti / Vulkan. Five new cases cover translation, opacity, default reset,
node-generation replacement and old-snapshot resubmission. `metadata.json`
contains 86 SHA256 source hashes, all checked against final files before
freezing. `verification.json` records counts. Root benchmark JSONs are the full
hardware runner's controls, retained for regression context, not attributed as
GPU speedups caused by this change.

## CPU executable comparison

The old executable was saved before implementation. The final executable was
built with the same `retained-runtime` feature set and identical CPU benchmark
source. Both are release builds with the same toolchain. Baseline source hashes
and binary SHA256 are in `baseline-metadata.json`; library files match the base
commit (the extraction file had CRLF checkout bytes). The new harness and Cargo
bin declaration were added before building either executable.

`abba/` contains four sequential runs in old → new → new → old order. Each run
has three rotating selective/full rounds, six warmups and twenty measured
events per mode/round. Each pooled version/case has 120 measured samples.
`execution.json` records timestamps, binary hashes, successful exits and zero
observed concurrent compiler processes, sampled approximately every 500 ms.
This does not establish an otherwise idle or controlled host. Per-run medians,
P95, total timing and raw sorted samples are preserved. `summary.json` pools
samples without dropping outliers.

Each fixture has 1,000 or 10,000 leaf nodes, each with nondefault transform,
clip, scroll and opacity: 4,000 or 40,000 sparse property entries. Transactions
alternate one leaf translation, one leaf opacity, or 128 contiguous leaf
translations, extracting exactly the changed leaves in selective mode. The
same persistent tree runs forced full extraction as a control. An original
snapshot remains alive; final items, property maps and damage are compared
outside timing. Publication includes replacement/retirement of the producer's
previous snapshot and paint-page directory copying. Transaction construction,
initial layout, consumer validation/planning, GPU work and native presentation
are excluded. Total columns include commit plus publication.

| Leaves / entries | Local change | Old publication median / P95 (µs) | New publication median / P95 (µs) | Old / new commit+publication median (µs) |
| --- | --- | --- | --- | --- |
| 1,000 / 4,000 | 1 translation | 59.0 / 126.6 | 3.4 / 6.0 | 60.0 / 4.4 |
| 1,000 / 4,000 | 1 opacity | 56.7 / 70.1 | 1.2 / 1.4 | 57.6 / 1.9 |
| 1,000 / 4,000 | 128 translations | 310.0 / 563.2 | 132.1 / 319.5 | 351.7 / 161.0 |
| 10,000 / 40,000 | 1 translation | 2297.9 / 5820.7 | 4.0 / 5.4 | 2302.7 / 4.5 |
| 10,000 / 40,000 | 1 opacity | 2662.7 / 5601.5 | 1.1 / 1.6 | 2667.5 / 1.7 |
| 10,000 / 40,000 | 128 translations | 2541.4 / 3290.3 | 155.0 / 245.9 | 2591.2 / 183.3 |

Check the exact total medians in `abba/summary.json`; these measurements support
a substantial local CPU publication benefit in dense property scenes. They do
not establish an end-to-end frame-rate multiplier. The change removes total
property count from snapshot cloning, but dirty-subtree extraction, persistent
lookups/path copies and the N/64 paint-page directory remain.

## Full-extraction cost and host drift

Full extraction became more costly in most controls. At 1,000 leaves, old/new
full medians are 1392.9/1987.3 µs (translation), 1314.6/1815.7 µs (opacity),
1399.2/1810.7 µs (128 translations): roughly 29–43% higher. At 10,000 leaves,
translation controls are 19683.7/22450.9 and 19833.3/22514.8 µs, about 14% higher.
An AVL record uses its own Arc allocation, with less compact storage and extra
pointer/atomic work relative to `BTreeMap`; this tradeoff is not eliminated.

There is material host-load drift despite no observed compilation during the
four runs. The 10,000-leaf old full-opacity per-run medians are 34159.4/21869.9
µs versus new 21565.8/21304.7 µs, so its pooled apparent improvement is not a
general full-rebuild benefit. Old selective single-translation per-run medians
are 4337.6/2011.0 µs versus new 4.0/4.0 µs; old selective opacity is
4393.5/2420.5 µs versus new 1.5/1.1 µs. Reported large ratios should be read with
these controls and the different allocation costs; they are not normalized
causal estimates. Memory footprint, sustained 60/120 Hz, energy, input latency
and cross-platform acceptance remain unmeasured here.

`preliminary/` preserves the initial incremental AVL builder and early paired
runs. Their full-extraction slowdown motivated bulk construction; they do not
describe the final implementation. Another attempt to start the second pair
was deferred when concurrent compilation was detected; it produced no measured
files. The final complete four-run comparison starts afresh in `abba/`.

## Reproduction

```powershell
mise exec -- cargo test --locked --offline -p gpui-box -p gpui-box-example-hello --features gpui-box/retained-text,gpui-box/retained-wgpu,gpui-box-example-hello/retained-wgpu -- --include-ignored
mise exec -- cargo clippy --locked --offline -p gpui-box -p gpui-box-example-hello -p gpui-box-rendering-perf --features gpui-box-example-hello/retained-wgpu,gpui-box-rendering-perf/retained-gpu --all-targets -- -D warnings
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/property-maps-check
mise exec -- cargo build --release --locked --offline -p gpui-box-rendering-perf --features retained-runtime --bin property-runtime-perf
# After all compilation has exited:
./target/release/property-runtime-perf.exe target/property-runtime.json
```

For old/new comparison, build this unchanged CPU harness against the pre-change
library in an isolated checkout, save the executable, then build the candidate
with the identical feature set. Do not compile while timing. Compare complete
outputs and full controls, retaining run order, raw samples and binary hashes.
