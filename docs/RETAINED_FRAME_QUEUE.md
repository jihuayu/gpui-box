# Coalesced visual frame mailbox

`retained::frame_queue::FrameQueue` holds at most one pending complete visual
FramePacket for one tree/window source. It is safe to share across producer and
consumer threads; the UI owner never exposes its mutable tree. Publishing a
newer revision replaces pending visual output and returns a wake intent only
on empty-to-pending transition. Taking the newest frame is nonblocking.

Equal revisions do not enqueue or wake, even after consumption. Older revisions,
foreign trees and invalid base revisions are rejected. A new UI tree uses a new
queue. Device-scale/surface-only demand remains separate: the gallery still
submits the latest packet when its surface changes without a logical revision.
The normal retained-WGPU gallery passes its immutable frames through this queue.

Each packet includes the complete visual snapshot. When coalescing skips the
renderer baseline, existing base-revision checks force full synchronization.
This mailbox owns no business, NativeView-lifecycle or resource-release commands;
such commands must never be routed through this discardable visual channel.
In-flight packet leases and GPU allocation retirement remain completion-driven.
Equal-frame submission now polls completions before returning, so a repeated
unchanged frame can release completed leases without new drawing.

The mailbox does not reduce the cost of producing each intermediate snapshot;
upstream scheduling can later defer extraction. It does not start a renderer
thread, install an OS wake callback or collect GPU completion automatically while
the host is entirely idle. It reports wake intent for a host scheduler to consume.

Verification passes 41 retained-WGPU core tests, including stale/equal/foreign
rejection, exact queue counts, immutable old content and cross-thread transfer
of 100 owner-produced snapshots without sharing mutable UI. Both normal-click
tests pass with hardware execution enabled, and Clippy/desktop builds pass.
The GPU suite passes 67 pixel scenarios. A producer emits 64 updates after an
initial consumed frame, coalesces 63 into one pending frame and produces one
wake intent. Consuming the latest forces resync, keeps at most three GPU
submissions in flight and matches full reference pixels. A weak-reference check
proves the initial snapshot is released after GPU completion.

[Frozen evidence](performance/2026-11-11-queue/README.md) records this source and
hardware result. Full device/surface-loss, resource epochs and idle-completion
host scheduling remain in the [complete goal](RETAINED_IMPLEMENTATION_PROGRESS.md).

The [source-delivery follow-up](RETAINED_SOURCE_DELIVERY.md) moves coalescing
upstream of reactive commit and extraction for background visual-state values.
It uses an awaited host waker rather than producing every intermediate packet.
This does not add a renderer thread or GPU completion wake while entirely idle.
