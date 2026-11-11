# Retained runtime hardware validation

This artifact records the [retained GPU integration](../../RETAINED_GPU_INTEGRATION.md)
on GTX 1650 Ti / Vulkan. `validation.json` contains 16 successful same-backend
pixel comparisons. The PNGs preserve each scenario's actual GPU output.
`adapter.json` identifies the device and driver. `metadata.json` identifies the
base commit, toolchain and hashes of the measured source files.

The test executable also asserts local upload payloads, adjacent range merging,
10,000-object translation without layout or command recording, same-frame no-op,
stale-frame rejection, dropped-base reconstruction, bounded submissions and
reset/reallocation behavior. These checks run before its successful exit.
The 32-update in-flight scenario delays readback until the final frame. Separate
CPU tests validate retirement using controlled completion serials.

Reproduce from the repository root.

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/retained-runtime-gpu
mise exec -- cargo test --locked -p gpui-box-example-hello --features retained-wgpu normal_click_reaches -- --ignored --nocapture
```

This artifact verifies correctness and upload accounting. The earlier
single-color benchmark remains the hardware timing artifact. Native surface
presentation, sustained animation timing, forced device loss and resource budget
qualification continue in the active implementation goal.
