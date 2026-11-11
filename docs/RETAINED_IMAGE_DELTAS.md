# Base-relative image draw updates

Image publication now follows the same base-relative ownership model as glyph
updates. `ImageUpdates` contains reset, removed draw IDs and changed images.
Sequential FramePackets send only changed items/removals, leaving every other
image draw, texture and geometry binding intact. Missing bases, new trees, scale
changes and backend reset supply the full current image set. A logical resync on
an intact backend reuses matching textures. Surface-only resize refreshes retained
viewport uniforms even when the producer emitted no item updates.

The backend keeps a per-texture draw reference count and live decoded-byte count.
It validates incoming geometry, dimensions, exact byte length, immutable resource
identity, duplicate/conflicting updates and the final live budget before mutation.
Preparation applies net ownership changes before evicting zero-reference textures;
same-frame swaps and shared-owner removal do not destroy resources that remain
live. Unchanged draws are no longer revalidated/reprepared. The backend still
constructs a full live draw-ID set for composition validation and collects live
GPU leases; those scans and physical memory accounting remain open.

`QuadSubmission.image_batch_updates` reports incoming image records. The hardware
suite passes 128 pixel scenarios: new cases cover 80 unchanged images under an
unrelated color update (0 records/0 geometry bytes), one moving image (1 record,
48 geometry bytes), shared-owner removal and skipped-base resync without texture
upload. Direct backend assertions additionally cover atomic swaps, mutable-ID
spoof rejection, duplicate/conflicting updates/removals, invalid geometry/bytes,
reset/removal conflict, valid submit after rejection, surface-only resize and
last-owner eviction followed by reupload. Existing in-flight/image/font checks
pass. Core tests pass 54 cases; ten real-font/normal-click tests, desktop WGPU
build and combined/feature-specific Clippy `-D warnings` also pass.

[Frozen evidence](performance/2026-11-11-image-deltas/README.md) compares delta
input with a full image-list reset on the same persistent consumer, with warm
shared textures, identical damage and draw behavior. At 800 images, moving one
image has median CPU submit 3,908.7 µs versus 4,411.4 µs full input. At 80 images,
an unrelated color update is slower in this run (1,181.6 versus 1,144.2 µs).
P95 and completed-work variation are substantial; zero/one input records are
stronger evidence than a universal latency improvement. Full composition planning,
per-image bindings/draws and completion leases still run. This is not image
batching/atlas support, native presentation, physical budget enforcement or an
end-to-end FPS claim.
