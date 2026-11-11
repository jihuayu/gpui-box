# Owned measurement evidence

This freezes the [owned measurement stage](../../RETAINED_OWNED_MEASUREMENT.md).
`measure-benchmark.json` contains raw CPU phase timings and callback counts.
`metadata.json` identifies that run's sources and machine. `gpu-validation.json`
contains all 54 successful pixel cases; `gpu-metadata.json` and `adapter.json`
identify their measured sources/device. Four `measure-*.png` images preserve
the new scenarios. Recorded hashes were checked against final measured source
before copying.

The CPU comparison includes retained commit/layout/frame extraction versus fresh
tree construction/initial layout/frame extraction. It excludes GPU, presentation,
text shaping and fresh tree destruction. Geometry, clips and commands are checked
after each update. Leaf callback counts are not full Taffy node-visit counts.
