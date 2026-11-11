# Downstream changes

The reference source remains Zed revision
`f8cfc7ed9e57503def9479138894f3139f5400cc`. `UPSTREAM.json` records the original
copied bytes. Its hashes intentionally describe the source baseline.

## Android M0

`crates/gpui/src/gpui.rs` adds Android to the existing priority-queue module and
re-export target gates. The Android platform uses the same priority scheduler
as upstream's Windows, Linux, and web backends. No queue algorithm changes are
needed. This is suitable for an upstream Android enablement patch.

The new Android backend is maintained as distribution-owned
`crates/gpui-android` at the repository root while its JNI/SurfaceView contract is
developed. This keeps experimental integration separate from the copied source
baseline. The official GPUI trait implementations and renderer remain path
dependencies on this snapshot. Existing upstream backend files are retained.

Verification on 2026-11-11 found 410 of 411 source entries matching their original
hashes. `crates/gpui/src/gpui.rs` is the single intentionally modified entry.
The reduced workspace manifest and materialized license links are the existing
initial-import adaptations.
