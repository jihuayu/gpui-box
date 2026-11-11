# Isolated layer hardware evidence

This freezes the [group-compositing stage](../../RETAINED_GROUP_COMPOSITING.md).
`validation.json` and 43 PNGs record passing pixel scenarios; each entry names
its oracle. Effect-free frames use the original GPUI full path. Effect frames
use a fresh full isolated renderer, supplemented by analytical overlap/nesting
checks in the hardware executable.

`layer-benchmark.json` records cached versus uncached isolated opacity animation.
`damage-benchmark.json` repeats Full/Damage/Auto controls. Both time completed
work without readback or native presentation. The gallery's readback bridge is
therefore outside the performance claims. `adapter.json` and `metadata.json`
record device, toolchain and measured source hashes, verified against the final
sources before this artifact was frozen.
