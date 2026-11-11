# Frame queue evidence

This freezes the [frame queue stage](../../RETAINED_FRAME_QUEUE.md).
`validation.json` contains 67 successful hardware pixel cases. The new
`queue-latest-resync` record includes 63 coalesced visual frames and one producer
wake intent; its image is retained here. `metadata.json` records verified source
hashes/build identity; `adapter.json` identifies the GPU. Queue validation also
asserts completion-driven release of the old FramePacket allocation.
