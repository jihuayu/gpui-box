# Reduced repeated lifecycle draft scans

This follows [atomic lifecycle implementation](../2026-11-12-lifecycle/README.md)
on base `987b49a`. Core retained-text/WGPU still passes 104 tests, gallery/font
passes 12 including real hardware clicks, and integrated Clippy passes
`-D warnings`. All 81 candidate runner source hashes match final files. The
candidate hardware suite passes 157 pixel cases; all four later comparison runs
also pass all 157 cases.

Three repeated operations were removed without weakening full preflight:

- A transaction marks whether child-list patches are pending. Appending a new
  draft node now skips replaying every earlier non-topology property patch.
  Pending child-list writes still settle before topology operations.
- New binding endpoints are indexed in a set, replacing linear conflict search
  through all prior draft bindings. Cancellation keeps that index consistent.
- Retiring logical scopes uses the ordered `(scope, key)` range for added state
  and effects, instead of repeatedly scanning every newly added entry.

These remove repeated draft scans; whole-arena topology validation, logical
ordering, parent child-list cloning, binding retirement and frame extraction
still remain. This is not a sublinear whole-event update.

## Executable A/B/B/A comparison

The baseline release executable was saved before editing, with its recorded
source metadata; the candidate was built after regression checks. Both use the
same frozen benchmark. They ran sequentially in `old-1`, `new-1`, `new-2`,
`old-2` order, with no concurrent compilation. Each executable runs the full
pixel/benchmark suite. Each lifecycle benchmark has three rotating rounds per
mode, ten warmups and forty measured events per round. Each version pools 240
draft samples per case. Binary hashes and raw round data are preserved in
`abba-summary.json`, `baseline-metadata.json` and the four run directories.

| Replaced scopes | Old draft commit pooled median / P95 (µs) | New draft commit pooled median / P95 (µs) | Old / new draft total pooled median (µs) |
| --- | --- | --- | --- |
| 1 | 196.95 / 359.3 | 178.85 / 299.7 | 543.05 / 499.4 |
| 16 | 402.2 / 692.8 | 348.3 / 658.4 | 789.55 / 687.55 |
| 128 | 2104.65 / 3088.1 | 1622.55 / 2499.1 | 2710.55 / 2098.1 |

Pooled draft commit medians are approximately 9%, 13% and 23% lower. This is an
observed local result with material host-load drift, not an environment-adjusted
causal estimate. For the 128-scope case, old draft per-run medians are
2418.7/1865.1 µs versus new 1582.6/1685.3 µs, while unchanged sequential-control
medians also drift from old 1029.0/835.0 to new 798.0/754.5 µs. The controls and
per-run medians must accompany interpretation. No GPU/host latency, sustained
cadence, energy or general throughput claim follows from these CPU samples.

An earlier candidate run was noticeably slower in both the unchanged sequential
control and draft. Its results are preserved in the root JSON files, motivating
the executable comparison rather than selectively presenting that initial
run or comparing only against a historical run. Unified draft remains more
expensive than the sequential APIs in these controls, because it validates and
publishes whole lifecycle events atomically. Further optimization and the full
[RFC acceptance goal](../../RETAINED_IMPLEMENTATION_PROGRESS.md) remain open.
