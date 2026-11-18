# Persistent row-height index: CPU A/B and full-path verification

Date:2026-11-19. Base:e9f39b4fb480489972f39ff5931ec87baa7de95d.
Worktree:`D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch:`codex/rendering-performance-research`.
[Decision/costs](../../ADR_RETAINED_HEIGHT_INDEX.md) ·
[Public API](../../RETAINED_VIRTUAL_LIST.md).
The complete RFC remains active;99% is figurative, not a statistical gate.

## What changes

Previously, with_heights coalesced edits but rebuilt every global prefix. Now
all metric consumers share a persistent sum index: implicit uniform ranges,
balanced shared branches and leaves with at most64 exact heights/local prefixes.
Only changed paths/leaves are copied. This is an indexing change; Taffy remains
the ordinary row-layout solver and the existing source transaction owns adoption.

Initial variable construction still costs O(N), retains O(N) worst-case data,
and can cost more memory than the old8-byte/row prefix. At a nonuniform leaf the
raw arrays cost4-byte heights +8-byte prefixes, plus tree/Arc/allocator overhead;
these are representation sizes, not measured process-memory deltas. Dense changes
and releasing a complete unshared old index can cost O(N). Small lists with many
changed rows can be slower; query pointer traversal is also a trade-off.

## Correctness controls

Six new core controls cover former-flat-reference window equality at neighboring
f32/exact row boundaries over0/1/2/63/64/65/127/128/129/257 rows, repeated sparse
edits, fresh/dense equality,1025 fractional rows/reveal, invalid/overflow rollback,
shared untouched intervals and reversion to uniform storage. For100,000 nonuniform
rows, changing three adjacent heights creates fewer than40 new nodes while
sharing all but fewer than40 old nodes. First sparse fixed-range change creates
fewer than30 nodes. Tests inspect actual Arc identity; they do not infer memory
bytes or end-to-end timing from node counts.

A distant keyed anchor at1,000,005px changes to1,000,035px after three preceding
heights add30px, preserving key50,000/inset5px. Another later edit changes total
but not this anchor. No payload factory runs; dataset keys/factory remain shared
and the old immutable metric snapshot remains unchanged.

Existing core controls exercise measurement/source/frame failure rollback,
FollowEnd/current scroll and bounded next-turn measurement. Real-Noto automatic
height controls now use160 rows (above the index leaf bound), widths100/65/110
and1.25scale; complete CPU/GPU images compare with fresh ordinary full Flex.
The GPUI host control checks the actually installed complete CPU image and
matching input scene. Host integration is not OS input injection or display cadence.

## CPU measurement method

`virtual-list-metrics-perf` is a release-only-purpose harness using the public
metric API and an independent copy of the previous flat-prefix algorithm.
Both create successive immutable changed snapshots, including dropping the prior
current snapshot; no dataset keys, components, sources, layout or renderer are timed.
Each control alternates old/new execution order;32 samples exclude two warmups,
with100 changes/sample for1,000 rows and12/sample for larger lists. Updates1/8/128
are spread every13 rows around the midpoint. Separate complete-window results
are checked equal before timed1024-query batches; query order also alternates.
Reported p95 is diagnostic sample distribution, not an acceptance gate.

Environment: AMD Ryzen7 4800H, Windows MSVC, Rust/Cargo1.98.1, locked/offline,
standard release profile. Timing excludes compilation. Raw final independent
repeats accompany this file; compilation/warmup exploratory results are not used
as final evidence. A query can regress while sparse updates improve; neither
metric is an FPS or input-to-display measurement.

Two final repeats ran the already-built binary after cargo/rustc/link processes
had finished. Raw [repeat1](metrics-r1.json) and [repeat2](metrics-r2.json) retain
all sample summaries. Binary SHA256:
`758659502dc7b7c9c59ace1bc4e9893e9f3bc37d0da69ad94907ba3e93f15450`.

| Rows | Changed heights | Old prefix p50, us | Persistent index p50, us |
| --- | --- | --- | --- |
| 1,000 | 1 | 2.90–2.92 | 1.05–1.06 |
| 1,000 | 8 | 2.90–2.93 | 1.52–1.52 |
| 1,000 | 128 | 6.80–6.84 | 13.32–13.42 |
| 100,000 | 1 | 269.48–273.35 | 1.38–1.81 |
| 100,000 | 8 | 424.75–441.10 | 2.58–2.68 |
| 100,000 | 128 | 431.13–448.43 | 23.57–23.70 |
| 1,000,000 | 1 | 4054.98–4091.61 | 2.89–2.92 |
| 1,000,000 | 8 | 4025.67–4253.15 | 3.29–3.58 |
| 1,000,000 | 128 | 4092.29–4146.73 | 24.89–25.82 |

For8 changed heights, complete-window query p50 regresses31→58ns at1,000 rows,
115–121→320–334ns at100,000 rows and618–634→792–891ns at1,000,000 rows.
The small-list128-change control is approximately2x slower. These are accepted
explicit costs of shared immutable sparse revisions; no scene threshold or
second algorithm is introduced to hide them. All final per-row heights/totals
and each complete-window result are compared before query timing.

Final retained-only786passed/1ignored; text/WGPU821passed/69ignored; each15compiler
doctests. Hello94passed/38ignored. Explicit measured-Noto GPU1passed,3 complete
images on GTX1650Ti/Vulkan/driver571.96; the160-row natural-height full Flex
reference covers branch-backed metric snapshots. Library and harness Clippy
with-Dwarnings, CPU desktop/retained-gpu harness checks, scoped rustfmt/diff,
1,155local links across300documents and five inherited WIP hashes pass.
Ignored cases do not count as executed GPU validation.

## Reproduction

```powershell
cargo +1.98.1 run -p gpui-box-rendering-perf --bin virtual-list-metrics-perf --features retained-runtime --release --locked --offline
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_measured_virtual_list_real_noto -- --ignored --nocapture
```

## Remaining scope

Private live Taffy copies, ordinary source/candidate/producer costs, full initial
key indexing and reorder/delete index rebuilds, offscreen estimated heights,
measurement invalidation granularity, sticky/horizontal/all-data accessibility
and independent native presentation remain. No sustained60/120Hz, byte-memory,
power or whole-app tail-latency conclusion follows from this isolated CPU A/B.
Validation logs are in `target/height-index-validation/`.
