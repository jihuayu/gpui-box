# Owned paragraph measurement evidence

This freezes [paragraph measurement](../../RETAINED_PARAGRAPH_MEASUREMENT.md).
`metadata.json` identifies measured CPU sources, font SHA256 and compiler.
`paragraph-benchmark.json` retains all sorted timing samples. Source hashes were
verified against the measured files before copying these results.

The font is bundled Noto Sans CJK SC, with system-font discovery disabled.
Content mixes Latin, a combining accent and Chinese. For each input and mode,
three rounds rotate execution order; each round discards six warmups and keeps
thirty samples. The source font system remains shared. Owned snapshots are
created before timing. An independent snapshot supplies expected geometry
outside timing, and every timed result is checked against it.

| Mode | 460 UTF-8 bytes median / P95 | 4,600 UTF-8 bytes median / P95 |
| --- | --- | --- |
| Owned four-width cache hit | <1 / <1 µs | <1 / <1 µs |
| Owned cache miss, distinct widths | 18.6 / 19.8 µs | 173.3 / 201.0 µs |
| GPUI warm window cache | 0.4 / 0.7 µs | 1.2 / 1.3 µs |
| Fresh GPUI window cache, shared font system | 318.6 / 394.0 µs | 2,677.4 / 3,727.4 µs |

Raw owned-hit readings are 0.1 µs and are near the timer's resolution; avoid
precise speedup claims at that scale. Hit/warm/fresh modes alternate 240/480
logical-pixel widths. The miss mode uses unique widths from 240 through 347,
so its results characterize different-width wrapping rather than an exact
same-constraint A/B against the cold or warm modes. Fresh window caches require
shaping and wrapping; the font database and resolver remain warm. GPUI paths
also clone text and produce decorated output; the owned hit returns a shared
existing layout. This is not a full application lifecycle comparison.

No glyph rasterization, GPU upload/draw, input latency, UI extraction or native
presentation is timed. Cache-miss cost remains proportional to paragraph work.
The source includes a four-result bound and a weak-reference eviction assertion.
Returned layouts can outlive eviction, as expected for immutable snapshots.

The `gpu-*` JSON files record a later 83-case hardware regression of the
quad/image renderer after this integration. They do not test retained glyph
pixels. Separate shared-example tests use real font shaping through a normal
click and compare resulting colored geometry with full GPU output.
