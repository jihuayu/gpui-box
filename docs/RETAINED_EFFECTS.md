# Post-commit effects and cleanup

`ReactiveUi::effect` and keyed `LogicalUi::effect` observe declared writable or
computed dependencies after a successful outer transaction. Registration runs
once immediately. Later runs happen synchronously after source values and the
render transaction have been published, before the commit result returns to
the host. The host still produces the owned frame during its normal prepaint
phase. Effects are UI-owner observations, not GPU completion callbacks.

Changed sources collect their effect IDs into one ordered set. An effect with
multiple changed dependencies runs once with all their final committed values.
Equal computed output does not trigger an observer depending only on that
output. Directly observing a changed writable source still triggers it even
when a downstream computed output is equal. Intermediate transaction reads do
not run effects; a final restore produces none. Returned transaction errors,
compute failures, sink validation failures and rolled-back nested scopes run
neither observers nor their cleanup.

Each observer returns optional `EffectCleanup` (`Box<dyn FnOnce()>`). The old
cleanup runs before the next observation. Explicit disposal, logical scope
removal and runtime drop run the last cleanup once. Effect IDs include runtime
identity and a never-reused serial. Effect disposal unregisters reverse edges;
source disposal rejects surviving observers with `SignalInUse`. Scope removal
preflights all removed sources/effects together, rejects observers in surviving
scopes before mutation, and cleans removed observers before any input is
retired. Keyed reorder retains the original callback, with dependency-set
validation on key reuse.

Observers receive read-only declared inputs. They do not receive mutable UI
access and cannot enqueue recursive source writes through this API. The
callbacks are intentionally infallible: a panic or external side effect after
publication cannot roll back a committed UI. Cleanup/callbacks must not panic
or try to reenter the runtime through captured aliases. No asynchronous effect
executor, cross-event scheduler, dynamic dependency tracking or priority system
is introduced. General effect-triggered writes require an explicit future host
queue rather than mutation during publication.

`effect_runs` and `effect_cleanups` in the commit/stats count reruns and replaced
cleanup from successful transactions. Initial setup, explicit/scoped disposal
and runtime-drop cleanup are outside transaction counters. Effects alone do
not request visual frames. The gallery uses a keyed observer to display its
committed material color; normal-click tests assert that value and one effect
run, alongside unchanged idle-render behavior.

## Verification

Four additional core cases cover multi-dependency deduplication, final-value
observation, cleanup order, returned errors, nested rollback, intermediate-read
restore, foreign/stale IDs, undeclared reads, source disposal guards, keyed
scope retention and cross-scope dependency removal. Runtime drop cleanup is
checked against explicit/scoped disposal to avoid a second call.

The hardware computed route now attaches an observer to bucket, color and
translation. It verifies their final consistency, one observation/cleanup for
the mixed batch, no observation for equality/read-restore/invalid sink, and final
cleanup when the logical runtime drops. Existing pixel cases continue to use
the ordinary full-renderer references.

`computed-runtime-perf` also writes `effect-benchmark.json`: zero, one and 1,000
observers; source-equal, derived-equal and changed-output events. Twelve warmup
samples precede 180 measured samples. Short events are averaged over 1,000 or
100 events per sample. Each changed observer reads its dependency, increments a
counter and allocates a cleanup that increments a second counter; that callback
work is included. Setup, final disposal, rendering and GPU are excluded. Exact
run/cleanup counts and zero frame requests are asserted. This reports CPU cost,
not an application speedup.

[Frozen evidence](performance/2026-11-12-effects/README.md) records final source
identities, samples, hardware results and verification boundaries. The
[full RFC goal](RETAINED_IMPLEMENTATION_PROGRESS.md) remains active.
