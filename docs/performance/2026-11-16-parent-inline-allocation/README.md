# Independent custom-parent inline allocation — 2026-11-16–17

Windows x64 / Rust 1.98.1 / locked offline. No Taffy source or GPU record format
changed. This stage adds a framework protocol, not a timing benchmark.

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib parent_layout::inline_tests -- --include-ignored --nocapture
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
```

Results: retained **314 passed**; text/WGPU **347 passed, 8 ignored**;
hello **23 passed, 2 ignored**; Clippy `-D warnings` passed. Focused new framework
run **10 passed**, including nine regular cases and the default ignored hardware
case explicitly enabled. No vendor test suite was rerun for unchanged sources.
Existing retained-only SpatialUpdate unused fields and Windows macro linker
informational warnings remain. Cargo GC rebuilt dependencies during the first
combined check; it did not change the test result.

Nine regular framework cases cover:

1. Enforced width and percentage basis reach real measurements and final layout
   despite an authored exact width 999 and a general parent's unbounded request.
   Widths 80/120/80 assign child widths 40/60/40 and nested percentages 20/30/20.
   Snapshot equality includes phase identity.
2. Scoped responsive rules in both resolved/local atomic edits below a custom
   width phase select 10/30-high children at widths 80/140/80/140. Host width is
   half the available parent width; every CPU pixel equals an independent
   ordinary forced-full Flex tree, hits and old-frame ownership remain correct.
3. Nested custom phases compose; standalone responsive local output reacts to
   resize and phase replacement between calls. A half-width child then another
   half-width host has the expected threshold, content height and parent height.
4. An inline builder reads the actual custom-assigned 40/70/40 widths and derives
   content heights. Removing the protocol after observation rejects publication.
5. Invalid slot count, NaN width and negative percentage basis reject the entire
   source/layout/frame transaction without publishing new source/statistics/frame.
6. Arbitrary custom parents and ancestor aspect feedback remain unsafe query
   hosts; no unchecked opt-in Boolean authorizes either.
7. A query-derived computed ParentLayout sink on its own ancestor rejects as a
   dependency cycle before rule registration. The original expectation of a
   later failure was corrected: earlier rejection is the existing DAG contract.
8. Equal-numeric snapshot replacement or parent child insertion after either a
   responsive observation or inline-builder observation rejects. Original local
   frame revision remains; numeric equality cannot erase algorithm/topology inputs.
9. A sibling order rule on a fixed custom root precedes a slot-width consumer
   even when registered later. Index-based slots are 40/70; reordering changes
   the query host from initial 70 to 40, and its height stays in the narrow branch.

The hello regression uses real bundled Noto Sans CJK SC shaping/rasterization.
Widths 100/180/280/100 give query hosts 50/90/140, then query-selected padding
0/5 yields actual paragraph slots 50/80/130/50. The keyed inline branch reuses or
retires with the breakpoint, content height and footer derive from real line
count, and every CPU glyph pixel matches a fresh ordinary forced-full Flex tree.
Old output reproduces pixels after UI/font/raster owners are dropped.

Explicit GPU: **NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96**. Four actual
LocalFrame submissions at widths 80/140/80/140 join the custom allocator,
scope-owned ContainerQuery and inline builder. All GPU pixels equal a fresh full
ordinary Flex reference, with explicit leaf hit testing and parent width checks.
This is that adapter's output correctness, not modern desktop GPU heavy-effects
or visible presentation/continuous 60/120Hz/tail-latency qualification.

Costs are an optional Arc per snapshot, per-callback O(children) allocation and
validation, per-observation custom ancestor/ordered-child copies and final checks,
query ancestry scans plus the existing full preview/candidate work. No runtime
speedup, lower memory or smoother CPU delivery follows from these tests. The
[contract/ADR](../../RETAINED_PARENT_INLINE_ALLOCATION.md) and
[full progress table](../../RETAINED_IMPLEMENTATION_PROGRESS.md) retain gaps.
