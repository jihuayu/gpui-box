# Windows shared GPU output, 2026-11-12

Source base: `af4253c`. NVIDIA GeForce GTX 1650 Ti, DX12 driver `32.0.15.7196`,
native window scale 1.5. The original headless validation remains Vulkan.
Five native runs used binary
`31896790ba8f3031fba49e7b133d9038878946fccdd185d691a93a59487fdd19`.
All five 123-source manifests and both source ZIPs were verified. Compiler/other
GPU helper samples were empty: 4/4/4 observations for performance runs and 5/4
for delivery runs, with a nominal 500 ms sampling delay.

The table measures **owner commit/publication/submission plus completed export**.
It excludes native scene drawing, OS presentation, legacy sprite upload and
verification. Both controls use the same DX12 consumer, full redraw and opaque
root-color updates. Three independent processes × four alternating rounds ×
twenty timed samples give 240 samples/mode/size. Six warmups precede each block.
All 1,440 timed samples, round/execution medians and outliers are retained.

| Output | GPU / readback total median | Change | GPU / readback total P95 | GPU / readback export median |
| --- | --- | --- | --- | --- |
| 128×96 | 0.659 / 0.780 ms | −15.5% | 1.885 / 2.280 ms | 0.271 / 0.380 ms |
| 800×600 | 0.693 / 2.275 ms | −69.5% | 1.189 / 3.426 ms | 0.292 / 1.746 ms |
| 1600×900 | 0.946 / 6.430 ms | −85.3% | 1.869 / 8.728 ms | 0.427 / 5.852 ms |

All three execution medians improved at each size. The small case is sensitive to
host variability: GPU total execution medians range from 0.541 to 0.932 ms; its
gain does not establish a universal small-widget speedup. At 1600×900 the GPU
export execution medians range from 0.311 to 0.776 ms. No samples were filtered.
These measurements establish export overhead on this GPU, not a whole-UI frame
rate or input-to-present latency.

The final export workload performs 312 GPU copies including warmups per process,
with six extent-specific allocations and 306 reuses. At its last extent the pool
holds two textures, a logical payload of 11,520,000 bytes. It copies 803,831,808
GPU bytes in total. Output pixel readback and sprite-atlas pixel upload are absent from the
native painter. A 1600×900 CPU bridge reads 5,760,000 pixel bytes and subsequently
uploads a sprite when painted; that later upload is outside this benchmark.
These counts are payload observations, not driver/global device memory accounting.

The first allocation-per-frame prototype regressed 128×96 total median to
2.099 ms versus 0.680 ms for readback, and 800×600 to 2.329 versus 2.096 ms.
Its complete raw CSV, native framebuffer, source ZIP and manifest remain in
`initial-probe`. It had no process sampler and is exploratory evidence. The
completion-safe pool addresses its allocation cost; the initial regression is
not discarded or silently substituted with a faster run.

Each final native process passed 97,792 fixture/control pixel checks with channel
tolerance one and cached-scene equality. Its 40-update native sequence passed
122,880 further pixels with two allocations/38 reuses. It samples each new texture
before any producer finish/readback/poll, exercising the native ready-fence wait.
Fixtures retain old outputs after producer/rasterizer/tree destruction, test
changed extents, clipping, text/images/shadows/opacity and ordinary GPUI overlays,
reject eight-owner pressure and retry after release, and preserve native support
across explicit reset.

Both normal gallery delivery runs passed exact comparison of all 107,136 canvas
pixels against the actual D3D11 framebuffer. The GPU run made two shared output
copies/two allocations; the readback control made zero shared copies. Source
burst coalescing, equality, idle renders and idle completion retirement passed.
These launched windows were **hidden** and diagnostic draws were explicit.
Automatic visible presentation, DWM/swapchain latency and native damage/buffer age
remain unqualified.

The later device-recovery follow-up starts from local commit `439fada` and binary
SHA-256 `3dbf355955e19cbb0bf984a8acfff2f568c721ccf33585945a8f88a5a8860893`.
Three independent native processes rebuilt the real D3D11 platform resources with
a new device identity. Each retained an old snapshot without sampling it on the
new scene, advanced exactly one retained consumer epoch, submitted a complete
frame through `submit_for_window`, and matched all 3,072 replacement pixels within
one channel value. The existing 97,792 fixture/control pixels and cached-scene
replay also passed. This deterministic replacement does not cover a driver TDR or
physical adapter removal. The no-matching-DX12 readback fallback is implemented
but remains hardware-unqualified.
The same follow-up reran the complete retained hardware matrix; all 1,939
scenarios and 1,849 pixel comparisons passed on the Vulkan headless path.
Normal gallery delivery also passed in native-surface and readback-control
modes, with all 107,136 D3D11 canvas pixels matching in both runs.

The original full matrix passed 1,939 hardware scenarios/1,849 byte-exact pixel
comparisons. Core/gallery tests passed 135/12, retained-only tests 110, selected
all-target Clippy and the default desktop Clippy passed with `-D warnings`.
The source ZIP, raw CSVs, native framebuffer PNGs, logs and process records are
stored here; full-matrix PNGs are archived in `validation/pixels.zip`.
Run `python summarize.py` to recompute `summary.json` without filtering.

Full implementation boundaries and reproduction commands:
[native qualification](../../RETAINED_NATIVE_SURFACE.md),
[interoperability decision](../../ADR_RETAINED_NATIVE_SURFACE.md).
