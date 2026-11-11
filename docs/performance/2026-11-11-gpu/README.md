# Single-quad hardware GPU experiment evidence

The [report](../../GPU_QUAD_PATCH_RESULTS.md) summarizes this run on GTX 1650 Ti,
Vulkan, NVIDIA driver 571.96, with a 1200×400 offscreen target.

- `metadata.json` records the base commit and SHA-256 of the measured modified
  sources, toolchain, host and benchmark settings.
- `adapter.json` records the actual selected hardware adapter and driver.
- `samples.csv` records 5,427 individual updates, across 3 sizes, 3 paths,
  3 rounds and 201 samples. Timing fields use nanoseconds.
- `summary.csv` contains the derived per-round nearest-rank p50/p95/p99.
- `validation.json` records 18 successful whole-image equality comparisons and
  exactly 9 changed pixels per color change.
- `verified-*.png` preserves each validated image from the local-upload path.

Reproduce and independently verify the CSV with these commands from the root.

```powershell
./tooling/rendering-perf/run-gpu.ps1 -Output target/quad-patch-gpu -Samples 201 -Rounds 3
python tooling/rendering-perf/summarize_gpu.py docs/performance/2026-11-11-gpu
```

`full` uses the original full renderer, `retained_full` is a stable-buffer control
with full uploads, and `patch` writes one 160-byte quad. CPU timings cover a single
update. GPU timestamps cover the render pass. Device completion includes host
waiting. Pixel readback and timestamp mapping are excluded from the update spans.
All paths clear and draw the entire target each update.

The research source was modified after the recorded base commit and is identified
by the source hashes. The delivery commit includes those sources and copied run
evidence. Host timing variability, power state and serialized completion waits
limit extrapolation to a continuously presenting application.

After measurement, one two-line safety comment in `quad_patch_prototype.rs` was
corrected to clarify that the byte helper relies on the existing Quad ABI rather
than validating it. This comment-only correction changes that file's final hash.
The executable statements match the measured source. The other nine recorded
source hashes match the delivered files exactly.
