# Rejected reusable binding-slot dispatch

Status: rejected experiment; runtime restored to `903b936`. The complete RFC
goal remains active.

The candidate stored reusable private slots for binding closures and kept an
ordered `BindingId -> slot` map per source. Public IDs remained monotonic; stale
IDs were rejected, source order and atomic draft edits were preserved. 168 core
tests, strict Clippy and 117 mixed GPU comparisons passed. A churn test repeatedly
created33 reverse-order bindings, dispatched them, unbound every binding and
verified stale IDs remained invalid while the slot arena stayed at33 entries.

Exact reversed sustained pairs used10k leaves, ten effects, ancestor motion,
dense color epochs,800x600 scale1,60/120Hz, ten seconds/case and admission
after/before construction. All16cases /14,400ticks,19dense epochs, fresh pixels,
retired leases and strict analysis passed; hashes and process samples were stable.

| Round / Hz / admission | Commit median original → slots ms | Completion p99 / max original → slots ms | Losses | Longest unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: |
|1 /60 /after |7.565 →5.880 |29.51 /47.37 →25.49 /39.65 |26 →26 |2 →2 |
|1 /60 /before |8.689 →6.581 |29.07 /44.63 →29.38 /85.86 |29 →42 |2 →6 |
|1 /120 /after |7.746 →6.566 |26.29 /39.80 →25.21 /96.03 |134 →183 |5 →15 |
|1 /120 /before |6.958 →4.682 |22.06 /39.43 →18.72 /32.52 |95 →73 |10 →4 |
|2 /60 /after |5.628 →4.170 |21.90 /35.94 →23.71 /31.40 |24 →25 |2 →5 |
|2 /60 /before |5.585 →5.333 |25.39 /29.76 →24.80 /42.20 |22 →23 |2 →2 |
|2 /120 /after |5.306 →4.328 |18.49 /26.71 →24.09 /45.07 |72 →96 |6 →16 |
|2 /120 /before |5.746 →4.219 |20.09 /24.00 →17.44 /21.28 |85 →84 |11 →6 |

Although all eight commit medians improve, both120Hz after-work runs extend
longest unsatisfied intervals to15/16ticks and increase loss totals. The60Hz
before-work maximum also grows44.63→85.86ms. Median reduction is insufficient
for the anti-stall target, so restore the hash-table path and keep no new API.
These receipts are native completion upper bounds, not display/input latency;
no GPU/driver/OS cause is claimed.

`rejected-candidate.zip` preserves exact sources and patch; `evidence.zip`
preserves raw rows, strict summaries, hashes, process samples and mixed checks.
The restored release binary passes117 mixed GPU scenarios. Modern desktop GPU,
native present/input, CPU fallback and complete semantic/resource/cross-platform
acceptance remain open.
