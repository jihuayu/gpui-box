# Reactive update evidence

This freezes the [reactive stage](../../RETAINED_REACTIVE_UPDATES.md).
`reactive-benchmark.json` contains raw per-sample CPU commit timings, equality
and request-intent counts. `metadata.json` records its measured sources.
`gpu-validation.json` contains 44 successful pixel cases; the new signal batch's
actual image is `reactive-batch-10000.png`. `gpu-metadata.json` and `adapter.json`
identify the measured GPU source and device. All recorded source hashes were
checked against the final source before copying this evidence.

Earlier scenario images remain in the prior retained/properties/layers evidence
directories. This artifact retains the new signal case and the complete current
validation JSON. Equality timings are batch averages, changed-node timings stop
after tree commit, and frame-request intents are not native presentation counts.
