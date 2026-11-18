# Typed Command verification,2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `ad3615a9d4824d60ff45724d3387e03c622c66e0`.
See [contract/lifetime/costs](../../RETAINED_TYPED_COMMANDS.md).
Complete RFC remains active; no timing/FPS/memory/native fluency benefit claim.

## Controls

Eleven backend-independent cases:

- Capacity0 rejects; capacity2 admits2unresolved requests. Dispatch/take_payload
  does not release capacity. Busy returns original Save3; completion frees one
  slot, with FIFO IDs/payload and one result retained after terminal status.
- Accepted queued cancel completes Cancelled, removes dispatch and frees capacity.
- Running cancel wakes the worker's cancellation future and remains Running;
  explicit committed success can win, without overwrite by late cancellation.
- Worker-confirmed Cancelled and typed business failure are distinct outcomes.
- Receiver disposal completes queued ExecutorClosed; dispatched work can still
  succeed. Further admission rejects Closed.
- Last sender close wakes receiver; explicit close drains accepted FIFO, then
  ready=false. Independent running requests retain completion ownership.
- Running request dropped or caught worker panic reports ExecutorAbandoned,
  releases capacity and permits another request. Expected panic is diagnostic.
- Dropping a ticket does not cancel accepted business payload42.
- Receiver-ready and result futures wake through executor Wakers, and one owned
  result is consumed without changing terminal status.
- Worker thread consumes two intentions and commits Store revisions1/2 in order;
  tickets carry separate commit evidence, original snapshot0 remains readable.
- Cancellation races receiver disposal through a two-thread barrier64times;
  result is exactly Cancelled or ExecutorClosed, never a missing envelope/panic
  or duplicate terminal. Subsequent admission stays Closed.

One compiler control rejects sending another intention type. Existing read-only
Store/selector/source/context capability controls remain.

Core UI control extends the complete Store Row reference: command with expected
revision/owned model goes to a worker thread, commits Store and completes ticket.
Before apply_store the UI applied revision is still old; afterward it matches the
ticket's business revision.3model frames retain scope/node and compare every CPU/
GPU pixel to independent numeric full Flex. Earlier interim complete Store
revisions can be skipped visually without dropping a command result. Unrelated
projection-equal Store update still skips component selection. Original direct
Store controls reuse this harness and remain covered.

Real Noto control applies the same typed command/result/Store/UI boundary to
width100→140→80, long→short→long paragraphs/colors. Same scope/node persist and
component callback runs3times. Independent full Flex uses actual paragraph wrap
metrics for numeric root height; complete title LayoutResult/baseline and all
CPU/GPU pixels compare for3hardware images. Old glyph frame survives business,
UI/font/raster owner retirement. Existing direct Store text path stays covered.

## Results and limits

Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.

| Check | Observed result |
| --- | --- |
| No-default-feature Store/Command | 16passed,2compile-fail docs |
| Complete retained | 673passed,1ignored |
| Complete text/WGPU | 707passed,61ignored |
| Complete hello | 76passed,27ignored |
| Compile-fail docs | 8passed in both core configurations |
| Eleven new transport / core UI / Noto CPU | Included in complete suites |
| Explicit Command→Store→UI GPU | 1passed,3complete images |
| Explicit real-Noto command GPU | 1passed,3complete images |
| Library Clippy -D warnings | Passed |
| Complete retained-gpu harness check | Passed |
| Scoped format/diff /305local links across5docs | Passed |
| Five unrelated WIP hashes | Unchanged |

Explicit GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96. Complete suites /
hardware include final source code, including close/cancel race ordering. Counts
overlap; only explicitly executed ignored wrappers count as hardware evidence.
Existing unused SpatialUpdate/dependency linker warnings remain.

New Command module uses std only and no GPU /
retained dependency. Core/hello integration test harnesses extend prior Store
controls without changing solver/source/schema/render algorithms.

Capacity bounds unresolved queued+running requests, not payload bytes/completed
ticket results. Cancellation scans queued entries; command transport uses short
Mutex transitions and per-request completion allocation. No handler execution or
new thread/runtime is imposed; per-object scheduling remains application-owned.
Finite offscreen equivalence does not establish input/native presentation latency
or modern GPU/CPU cadence. No lint requirements are disabled.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --no-default-features --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_typed_commands -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_typed_commands_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
