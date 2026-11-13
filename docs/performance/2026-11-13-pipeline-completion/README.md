# Pipeline burst completion servicing

Status: host scheduling contract and measurement correction; full goal active.
No runtime renderer algorithm or public tuning API changed in this stage.

## Workload and controls

One binary, 10,000 leaves, ancestor motion, ten isolated effect groups, 800x600,
scale1. The source runs independently for ten seconds at60/120Hz. Every half
second changes all leaf colors; source coalescing retains the absolute epoch.
Each process compares admission before/after owner construction. Four eligible
processes use reversed waiter order, totaling16cases /14,400scheduled ticks.
Every case crosses19dense epochs, uses at most2GPU submissions in flight,
matches final pixels against fresh rendering and retires all leases. No
post-initial full resync occurs. The old source-only waiter remains a control.

Previously `wait_ready(source.ready())` could wake for completion and immediately
park again without processing its receipt. The harness now polls both channels
on its single owner, retires completed leases, and polls completion before
accepting source work when both are ready. Thread unpark preserves wakeups
across poll/park races; the35second wait is failure detection, not a polling
cadence. Normal native gallery integration already uses an independent owner
completion task. These measurements do not demonstrate that native integration
had the harness defect.

## Results

Numbers below are source-to-completion upper bounds and source-to-owner receipt
observation p99 in ms. Losses include superseded source ticks plus consumed
samples over8.333ms; longest is the longest unsatisfied source-tick interval.

| Round / admission | Old completion / owner p99 | Serviced completion / owner p99 | Old / serviced completion losses | Old / serviced longest |
| --- | ---: | ---: | ---: | ---: |
|1 / after |24.54 /29.62 |22.56 /22.58 |98 /85 |13 /6 |
|1 / before |22.26 /28.91 |23.39 /24.13 |85 /113 |3 /4 |
|2 / after |23.31 /32.84 |22.97 /23.00 |85 /106 |4 /14 |
|2 / before |22.34 /29.31 |24.77 /24.79 |96 /110 |3 /4 |

Serviced120Hz completion p99 stays22.56–24.77ms, max28.05–36.77ms. Both
before-construction pairs lose completion p99 and losses. Second after pair
also loses total/longest unsatisfied ticks. Completion servicing is necessary
for prompt receipt handling; it is not a demonstrated GPU execution speedup
or burst-stall solution.

At60Hz, old completion p99 spans26.68–32.21ms versus22.76–24.90ms serviced;
owner observation spans38.29–51.17ms versus22.80–24.94ms. Handling delay p99
falls from34.28–43.33ms to0.07–0.12ms. At120Hz it falls from24.08–25.74ms
to0.50–1.14ms. Source-only observation misses almost all source deadlines;
this is delayed owner bookkeeping, not evidence that almost all frames finish
late on the GPU. Coalesced covering receipts remain conservative upper bounds.

Serviced dense owner-span medians at120Hz are20.10–22.03ms, with19samples
per case. Phase medians are commit4.39–5.05ms, publication6.06–6.82ms and
submit/admission9.50–10.38ms. These are independently computed phase medians,
not a decomposition obtained by summing percentiles. Submission may include
driver waits. The same-row owner total independently exceeds the frame budget.
Capacity waits range0–8 per eligible case; layer-budget waits are0. There is
no stable evidence for choosing admission timing or expanding queue depth.

## Evidence integrity and validation

`captures.zip` preserves all three attempts, including the rejected first
attempt (25known compiler observations), the partially eligible attempt
(third process rejected for126observations), and clean reversed round2.
The analyzer lists rejected runs explicitly; it never treats them as eligible
performance evidence. A250ms known-process sampler cannot prove host/driver/
thermal/GPU isolation. Capture Rust sources and binaries stayed unchanged.
`capture-sources.zip` records the measured harness, `changed-sources.zip` the
final harness. Base runtime is6d1a13d. The binary hash is in each manifest.

The initial binary shadowed the `burst` boolean with its final miss-streak
counter in output metadata. Invocation, absolute per-row epochs and19epoch
checks establish the workload independently; final code fixes that metadata.
Old raw handling delay also used the receipt timestamp for equal-state samples
whose output completed before arrival. The analyzer retains the raw value but
derives meaningful delay as owner-observation age minus completed-output age,
matching `max(receipt, submit)` in the completion-age contract. Final code emits
the corrected span directly. Original raw data remains unmodified.

Final code makes dual-channel servicing the default, retaining
`--source-only-wait` only as a harness control. The measured earlier binary
selected servicing with `--service-completions`; its original commands remain
in manifests. Final changes also return a typed owner event so a regression
can wait on GPU completion while the source stays open and idle. It verifies
receipt coverage, zero remaining GPU/frame leases and source still pending.
Both harness tests and strict harness/test Clippy pass. Final release smoke
runs pass32cases:4burst default,4source-only and24general fractional-motion
cases, all with pixel/lease/flight assertions. These short post-change runs
are correctness evidence, not replacement ten-second performance captures.

Ryzen4800H / GTX1650Ti Vulkan is a regression host. Modern desktop GPU, larger
pixels, native present/display, real input and CPU fallback responsiveness
remain unqualified. No120fps or visible stall elimination claim follows.

## Reproduction with final sources

```powershell
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf
python tooling/rendering-perf/capture-burst.py target/pipeline-new --pipeline-pair
python tooling/rendering-perf/analyze-pipeline-burst.py target/pipeline-new
# A selected reversed round can be captured separately without overwriting evidence:
python tooling/rendering-perf/capture-burst.py target/pipeline-round2-new --pipeline-pair --round 2
```

Historical eligible analysis:

```powershell
python tooling/rendering-perf/analyze-pipeline-burst.py target/pipeline-burst-clean-20261113 target/pipeline-burst-round2-20261113
```

Next general work: reduce necessary transaction/publication/consumer work and
measure its boundary costs; preserve atomic lossless state and immutable shown
frames. Do not add application-selected caching/admission policies from this
host-specific experiment. CPU jobs must likewise yield independently of source
cadence, but that native integration still needs implementation and validation.
