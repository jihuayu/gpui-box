# Rejected contiguous ordered source bindings

Status: rejected experiment; runtime restored to `5be96a6`. Full RFC goal active.

The candidate replaced each source's `BTreeSet<BindingId>` with an ordered
`Vec<BindingId>`, using binary search for insertion/removal and contiguous
iteration for sink dispatch. Public IDs, source order, stale-ID errors, draft
rollback and atomic transactions were unchanged. 167 core tests, strict Clippy
and117 mixed GPU comparisons passed.

The valid reversed capture used10k leaves, ten effects, dense color epochs,
ancestor motion,800x600 scale1,60/120Hz, ten seconds/case and admission before/
after construction. All16cases /14,400ticks were captured; one covering-receipt/
owner-observation inversion caused strict full-chain analysis to reject the
summary, and raw rows remain preserved.

| Round / Hz / admission | Dense commit original → ordered ms | Completion p99 / max original → ordered ms | Losses | Longest ticks |
| --- | ---: | ---: | ---: | ---: |
|1 /60 /after |7.918 →7.689 |28.34 /38.01 →28.16 /34.59 |25 →23 |2 →2 |
|1 /60 /before |6.549 →7.833 |26.49 /37.58 →29.33 /33.21 |27 →35 |3 →4 |
|1 /120 /after |6.571 →7.328 |20.52 /31.93 →24.74 /53.35 |75 →84 |4 →10 |
|1 /120 /before |6.493 →7.064 |20.14 /27.10 →25.63 /51.34 |72 →104 |5 →8 |
|2 /60 /after |7.025 →7.498 |24.12 /29.86 →27.12 /34.38 |21 →27 |2 →4 |
|2 /60 /before |7.481 →8.619 |30.89 /59.08 →31.56 /38.71 |30 →34 |5 →2 |
|2 /120 /after |7.466 →7.723 |22.68 /32.01 →28.35 /42.53 |78 →140 |4 →14 |
|2 /120 /before |5.992 →8.753 |18.92 /22.27 →32.51 /51.65 |63 →164 |2 →8 |

Commit medians regress six of eight pairs. All four120Hz p99 values and loss
totals worsen; after-work longest streaks reach10,8,14 and8ticks. Restore the
original tree-backed membership. The timing inversion is preserved, not hidden
by tolerance. Receipts remain completion upper bounds, not display/input latency.

`rejected-candidate.zip` stores exact sources and patch; `evidence.zip` stores
valid/invalid captures, raw rows, hashes and correctness. The restored binary
passes117 mixed GPU comparisons. Native present/input, modern desktop GPU, CPU
fallback and complete RFC acceptance remain open.
