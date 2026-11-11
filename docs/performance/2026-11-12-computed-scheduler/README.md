# Computed scheduler comparison

This follows the [computed DAG implementation](../../RETAINED_COMPUTED_DAG.md)
and baseline `378ed23`. The static dependencies and observable transaction
contract are unchanged. A contiguous min-heap orders pending work, per-source
epoch stamps deduplicate enqueues without clearing an all-source bitmap, and
a rollback journal replaces the second ordered map of derived output. Failed
settlements and nested rollback use new epochs on retry. Stamps do not publish
source or render values. Epoch overflow returns an error before scheduling.

## Direct scheduler comparison

Two release executables were built in the same worktree, with only
`reactive.rs` and `reactive/computed.rs` switched to their baseline Git content
for the first build and restored for the second. Source hashes were checked
against the Git objects/current sources. Executable hashes and process order
are recorded in `comparison-metadata.json`. The shared benchmark, typed sink
code, fixture sizes and final equality assertions are identical. A test-only
regression added after those two builds does not enter their release code.

All builds finished before six timed processes: baseline/current,
current/baseline, baseline/current. Each process itself runs three alternating
mode rounds, twelve warmups and sixty measured samples/round. Short sample
averages and excluded stages are exactly those of the initial computed-DAG
benchmark. Each scheduler row below pools 540 samples from three processes.
Batch-average P95 is not single-event tail latency. External host load remains
uncontrolled; process medians and all raw samples are retained.

| Nodes | Workload | Baseline median / P95 (µs) | Journal median / P95 (µs) |
| --- | --- | --- | --- |
| 1,000 | source-equal | 0.063 / 0.070 | 0.066 / 0.079 |
| 1,000 | bucket-equal | 0.392 / 0.429 | 0.386 / 0.826 |
| 1,000 | one-group | 4.530 / 5.990 | 4.870 / 7.350 |
| 1,000 | all-groups | 888.350 / 1,313.600 | 825.900 / 1,761.300 |
| 10,000 | source-equal | 0.062 / 0.072 | 0.067 / 0.094 |
| 10,000 | bucket-equal | 0.398 / 0.608 | 0.393 / 0.618 |
| 10,000 | one-group | 4.590 / 7.240 | 4.520 / 6.670 |
| 10,000 | all-groups | 13,451.400 / 16,092.600 | 11,658.700 / 14,767.100 |

For 10,000-node dense changes, pooled median falls from 13.451 to 11.659 ms
(about 13.3%); individual process medians are 13.305/14.509/13.133 versus
11.235/11.323/12.875 ms. The 1,000-node dense case has mixed individual results
(baseline 0.868/0.915/0.880 ms; current 0.670/1.285/0.773 ms), so its pooled
change is not a stable per-process benefit. Sparse/no-op differences are small
or mixed. Dense DAG work remains slower than direct eager recomputation on
this fixture. This establishes a bounded CPU scheduling improvement, not a
general application FPS, input latency or energy result.

## Final-tree verification

Final CPU and GPU runs use the complete final source set recorded in
`cpu-metadata.json` and `metadata.json`. Core tests pass 62 cases; ten
real-font/gallery tests, including hardware normal-click pixels, pass.
Combined Clippy passes with `-D warnings`. The additional core test verifies
that a failed settlement restores staged derived values from a preceding
successful read before a corrected-input retry. The full hardware suite passes
139 pixel cases; all 10 CPU and 60 hardware source hashes were verified. Final
run outputs are retained. Unchanged GPU benchmarks are regression controls,
not measured improvements caused by scheduler changes.

The full RFC goal remains active. Effect scheduling/cleanup, cross-event
coalescing, dynamic dependency tracking, native GPU presentation and the other
acceptance items remain open.
