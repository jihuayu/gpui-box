# Rejected direct arena-to-GPU conversion probe

Status: candidate rejected and runtime restored tob956e30. Full goal active.
No new runtime API, switch or specialized fallback remains.

## Hypothesis and implementation

Existing arena `writes()` clones changed records into merged contiguous arrays.
Quad/Shadow consumers convert those arrays into GPU ABI records, constructing
another set of arrays. The candidate first discovers contiguous dirty runs,
borrows each live arena record and converts directly into a correctly sized
output vector. The local and resolved consumers use the same generic private
mapping routine. Ordinary public `writes()` delegates to identity cloning.

Generation checks, retirement, equality, successful-submit acknowledgement,
GPU bytes/ABI, preflight and painter order are unchanged. Reading dirty ranges
never acknowledges them. The candidate adds a two-stage per-run traversal and
changes allocation sizing in exchange for eliminating the intermediate source
array and geometric output growth. Reduced copying alone is insufficient reason
to retain it without a reliable benefit on the target path.

## Reversed pipelined comparison

Exact baseline isb956e30. Both binaries use the existing10k ancestor/effects
fixture,800x600 scale1, independent60/120Hz source, ten seconds per case, dense
persistent leaf color changes every half second and default independent
completion servicing. Each process runs admission after/before construction.
Original/candidate then candidate/original gives16cases /14,400scheduled ticks.
Every case accounts for19dense epochs, matches fresh final pixels, retires all
leases, keeps at most2GPU submissions in flight and has0post-initial resyncs.

Completion is a coalesced native-wait upper bound, not GPU execution time or
native visible display. Submission includes admission/driver work. Each dense
group has19samples; phase percentiles are computed from individual raw rows.

| Round / Hz / admission | Dense submit original → candidate median ms | Completion original → candidate p99 / max ms | Completion losses original → candidate | Longest original → candidate |
| --- | ---: | ---: | ---: | ---: |
|1 /60 /after |9.741 →9.896 |23.10 /26.27 →24.48 /31.16 |19 →19 |1 →1 |
|1 /60 /before |9.738 →9.786 |23.99 /27.76 →24.00 /28.39 |19 →19 |1 →1 |
|1 /120 /after |9.696 →9.508 |21.94 /36.86 →22.17 /27.84 |98 →90 |5 →5 |
|1 /120 /before |9.772 →9.557 |22.80 /29.59 →21.74 /30.40 |77 →89 |4 →4 |
|2 /60 /after |9.167 →9.338 |23.46 /27.31 →22.58 /31.79 |19 →20 |1 →2 |
|2 /60 /before |9.843 →9.701 |23.94 /38.55 →26.62 /30.03 |21 →19 |3 →1 |
|2 /120 /after |9.862 →10.006 |22.59 /29.81 →22.50 /26.56 |97 →94 |5 →5 |
|2 /120 /before |9.484 →9.700 |22.50 /27.95 →21.19 /27.64 |97 →82 |4 →3 |

Four submit medians improve and four regress, spanning about−2.2% to+2.3%.
The120Hz submit gains in round1 reverse in round2. First120Hz before case loses
completion deadlines despite better p99; second60Hz after case loses longest
interval and max despite better p99. Dense owner medians remain19.59–20.61ms
on the candidate, still well above8.333ms. No stable deadline/stall advantage is
established. This experiment cannot show that fewer intermediate copies are
always slower, nor that the differences are causal rather than host variance.
It does show insufficient evidence to keep the candidate's extra algorithm.

## Decision and next direction

Restore the original arena and both runtime consumers exactly, including the
candidate-only test. Preserve its exact sources in `rejected-candidate.zip`,
raw data/manifests/analysis in `evidence.zip` and local binaries under
`target/arena-conversion-rejected-binaries/`. Retain generic harness baseline
selection and candidate labels so subsequent experiments need no per-experiment
CLI mode. No application API is justified by this probe.

Next measure internal submission phases (record preparation, projected spatial/
primitive/resource preflight, cache invalidation, storage/upload, driver submit)
before choosing a larger algorithmic change. Maintain full validation/recovery
authority; do not bypass checks or add scene-specific routing to meet a number.
Native display/input and responsive native software integration remain separate
required work. Modern desktop120fps is still unqualified.

## Verification and evidence integrity

Candidate166core tests, strict library Clippy and75mixed GPU comparisons pass.
The new candidate-only regression uses source records that panic on cloning,
checks fragmented cross-word dirty runs, injected late conversion failure,
unchanged retry data, completion-retired slot reuse and stale handles. Existing
GPU tests exercise material failure/reset, fractional scale and mixed content.
Correctness did not decide rejection; measured performance did.

Both source/binary hash checks stayed stable through formal capture. All four
processes report zero known compiler/renderer conflicts at250ms sampling.
That does not establish host/GPU/power/thermal/driver isolation. Hardware is
Ryzen4800H / GTX1650Ti Vulkan, not modern desktop qualification. Rebuilt restored
runtime smoke evidence is recorded separately and is not a performance capture.

```powershell
# Save b956e30 dense-runtime-perf.exe as baseline; restore archived candidate sources.
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf --bin retained-runtime-gpu
python tooling/rendering-perf/capture-burst.py target/conversion-new --baseline target/arena-conversion-baseline/dense-runtime-perf.exe --candidate-name converted --pipelined
python tooling/rendering-perf/analyze-pipeline-burst.py target/conversion-new --implementation-pair --candidate-name converted
```

The working tree intentionally contains the restored runtime, so rerunning the
probe requires the archived candidate; running current sources as the candidate
would compare two baseline implementations. Exact binary hashes are retained
in the original manifest. No full-suite timing/performance claim is made here.
