# Vendored Taffy 0.15.0

This directory preserves the cached crates.io package for `taffy =0.15.0`.
`UPSTREAM.json` records its registry checksum, Git source revision and original
per-file SHA256 values. `LICENSE` was fetched from that exact source revision;
its source URL and SHA256 are recorded separately because the published package
did not include it. Taffy is MIT licensed. The original manifest, source and
examples are retained. The distribution workspace uses a crates.io patch to
select this source for both the persistent owner and GPUI, retaining the pinned
version and enabled layout algorithms. Its standalone Cargo.lock/target are
local test artifacts and ignored.

## Local changes

- Grid minimum outer contribution is floored at resolved padding+border even
  with explicit/automatic zero minima. Independent browser references exposed
  fit-content tracks smaller than their border-box floor; the standalone
  regression and all1,152framework Grid cases cover the correction. See
  [public Grid contract](../../docs/RETAINED_PUBLIC_GRID.md).

- Flex preserves authored-size provenance, distinguishes replaced/ordinary
  automatic minima and transfers preferred ratios through the actual ratio box.
  Explicit cross sizes and automatic content minima retain their own limits.
  See [contract and independent reference](../../docs/RETAINED_FLEX_AUTOMATIC_MINIMUM.md).

- ContentSize direct-child entry skips the queried node's own preferred keyword
  resolution; owned custom/ConstraintBox sizing suppresses its aspect suggestion
  in that mode. CustomLayoutInput exposes the invocation mode to pure/full plans.
  See [content-sizing contract](../../docs/RETAINED_CONTENT_SIZING.md).

- Independent CustomAllocationChildren now shares the unresolved item-style
  view, without child measurement. The framework captures effective sibling
  styles in final structural observations. See
  [allocation-style contract](../../docs/RETAINED_ALLOCATION_STYLES.md).

- CustomLayoutChildren::style returns owned unresolved item inputs from the real
  node style, including hidden/replaced classification. The allocation follow-up
  above extends the same view to independent full allocators. External low-level
  adapters default to an explicit unsupported error. No per-node storage/cache
  format is added. See
  [framework contract](../../docs/RETAINED_CHILD_STYLE.md).

- `set_exact_measure` records external content-measurement cache requirements in
  a sparse capability index. Complete input/output measurement and final-input
  guards apply through ancestors; clone/hidden/remove/clear/generation lifecycle
  is explicit. The framework uses this for pure generic content leaves. See
  [content protocol](../../docs/RETAINED_CONTENT_MEASURE.md).

- Optional custom-parent pure metrics avoid complete placement plans during
  ComputeSize and are checked against the final plan's size/baselines. The
  measure_node_with_input API forces ComputeSize with explicit parent bases and
  exact-cache/sticky-error handling; ordinary baseline/hidden probes may still
  use scratch layout writes, so framework public queries run on a clone. See
  [dry parent contract](../../docs/RETAINED_DRY_PARENT_MEASURE.md).

- Immutable opaque direct-child parent data lives in a sparse map with
  clone/hidden/reparent/remove/clear lifetime and identity-aware invalidation.
  CustomLayout allocation now receives a read-only count/data view, and measured
  parents can read the same values. No child style or measurement is available
  during independent allocation. Raw kernel implementors need the new allocate
  signature; framework count-only adapters remain supported. See
  [parent-data contract](../../docs/RETAINED_PARENT_DATA.md).

- Full size-contained nodes record real incoming final LayoutInput in sparse
  storage; intrinsic probes do not replace final observations, cached ancestors
  preserve them and hidden/style/remove/reuse/clear retire them. A sparse node
  index enables the existing complete-input cache mode before first layout,
  preventing known-size/soft-space cache aliasing from hiding observable builder
  inputs. No NodeData/GPU payload growth or performance gain is claimed. See
  [mixed Flex allocation builders](../../docs/RETAINED_MIXED_ALLOCATION_BUILDERS.md).

- First/last horizontal baseline sharing groups in Flex, separate final item
  metrics and container exports; singleton groups, wrap-reverse and RTL column
  fallback follow the corresponding edge. Cross-axis auto margins overflow at
  logical end instead of becoming negative at logical start. Block transports
  its last eligible child's baseline; ConstraintBox/custom LayoutOutput already
  carry both. AlignItems/AlignSelf add LAST_BASELINE and parse/serde support;
  raw Grid retains End fallback, not last-baseline sharing. Existing Baselines
  and persistent cache/layout formats suffice; transient FlexItem gains one f32
  and baseline group/measurement work increases. See the public
  [contract and evidence](../../docs/RETAINED_LAST_BASELINE.md).

- Owned custom invocations use a sparse exact measurement side table rather
  than the ordinary size-only cache or an unconditional ComputeSize bypass.
  Each visited node stores at most nine full LayoutInput/LayoutOutput records;
  full percentage/axis/definiteness/bounds and baseline/margin/overflow metadata
  are preserved. Arc copy-on-write shares unchanged stores across private
  previews. Dirty propagation, hidden/cache clear, remove and tree clear retire
  entries, including exact-only measured nodes. Ordinary per-node cache payload
  and public APIs stay unchanged. Per-tree selection, extra record/map memory,
  clone/write costs and global byte budgets remain explicit limitations. See
  [contract and validation](../../docs/RETAINED_EXACT_PARENT_MEASURE_CACHE.md).

- Constraint boxes record their final child-slot `LayoutInput` in a separate
  map, including empty boxes. Intrinsic probes do not overwrite it; ancestor
  cache hits retain it; hidden/style/remove/clear retire it. A default trait
  hook keeps custom layout adapters compatible. Framework construction runs
  outside the layout stack and requires size/layout containment plus final
  input revalidation. One input record per laid-out box adds memory/map cost;
  no Style size increase or speedup claim. no_std retains the existing
  SecondaryMap fallback. See [builder contract](../../docs/RETAINED_CONSTRAINT_BUILDERS.md).

- Explicit non-CSS `Display::ConstraintBox` selects a single-child parent
  protocol with per-axis Tight/Loose content allocation. It composes with CSS
  parents/children while applying hard child bounds inside real measurement,
  baseline, overflow and OOF publication. A sparse index validates queried
  subtree box arity/direct OOF children before layout writes. Display grows
  from 1 to 2 bytes in the tested build; String/Arc-str Style without balance
  grows by 8 bytes to 560/528. Balance-enabled sizes remain unchanged.
  CSS parsing adds no keyword. See the
  [nested contract](../../docs/RETAINED_NESTED_CONSTRAINT_BOX.md).

- Host/parent border-box `SizeBounds` is independent of authored min/max and
  soft AvailableSpace. `LayoutInput::size_bounds` participates in real root,
  leaf/Flex/Grid/Block/OOF sizing; child calls reset it, and exact parent sizes
  establish definite assignments. Host maxima can override padding floors;
  negative inner/positioning sizes floor to zero. One bounds mode per node
  cache rejects stale results without growing each entry key. High-level
  `compute_layout_with_bounds_and_measure_and_observer` validates inputs;
  existing entry points pass NONE. Complete LayoutInput literals and exhaustive
  TaffyError matches need migration. See the framework's
  [parent-constraint contract](../../docs/RETAINED_PARENT_CONSTRAINTS.md), including
  memory costs and the remaining nested ConstraintBuilder/structure work.

- `AvailableSpace::Unbounded` represents normal indefinite layout, separate
  from intrinsic MinContent/MaxContent. Flex uses hypothetical outer sizes for
  normal unbounded main sizing; real bounds constrain wrapping/growth. Grid
  handles indefinite track expansion without taking intrinsic-only branches;
  Block/float/text and arithmetic support the tag. The mixed cache key reserves
  a distinct bit payload. GPUI's compatibility enum, scale conversions and list
  measure adapters also handle the new variant. Downstream exhaustive matches
  must migrate; CSS parsing does not add a new keyword. The framework owns the
  [independent viewport and unbounded contract](../../docs/RETAINED_UNBOUNDED_LAYOUT.md),
  including remaining conformance and performance limits.

- Minimum/maximum styles in `Style` and `CoreStyle` now use `Size<Dimension>`.
  The shared intrinsic-limit resolver integrates keywords into actual Flex,
  root, legacy Grid/Block and out-of-flow algorithms, with ContentSize isolation
  and final Flex-width cross-axis remeasurement. This is a source API departure
  from upstream; explicit `LengthPercentageAuto` field values need conversion
  to `Dimension`. Numeric style helpers still infer the field type.
  `compute_leaf_layout` now requires `FnMut` rather than `FnOnce` because
  intrinsic probes may call the measure function repeatedly. GPUI and the
  measurement example reborrow mutable context per invocation. See the
  [intrinsic-limit contract](../../docs/RETAINED_INTRINSIC_LIMITS.md) for costs,
  migration, browser differences and incomplete conformance.

- `src/tree/taffy_tree.rs` adds a generic, statically dispatched callback at
  `set_unrounded_layout` and a `compute_layout_with_measure_and_observer` entry
  point. The existing entry point supplies a no-op callback. Observations
  include equal and transient writes, and run before the unrounded layout is
  stored. Callbacks must not panic or mutate the tree. Consumers must
  deduplicate IDs and compare final layouts after computation. When rounding
  is enabled these observations are not final rounded results. The retained
  owner explicitly disables rounding.
- `src/tree/layout.rs` adds `Layout.baselines`, populated from the final
  `LayoutOutput` by root, Flex, Grid, Block/float and out-of-flow publication.
  Hidden layouts clear it. It stores horizontal baselines relative to the
  border-box top, independent of cache lifetime. `compute/mod.rs` rounds each
  baseline in absolute coordinates before subtracting the rounded box origin.
  This adds two `Option<f32>` fields to each final layout; the framework removes
  its separate leaf-only published baseline. No extra tree traversal or cache
  lookup is added to publication.
- Grid final positioning now passes its child's actual final first baseline
  to container propagation. Previously, rows with zero or one baseline-aligned
  item skipped the preliminary baseline pass and could incorrectly substitute
  the child's bottom edge; rows with multiple participants could count the top
  margin twice. Track sizing and alignment shims retain their existing roles.
- Two test-only feature assumptions are corrected: the calc pointer test is
  gated on `calc`, and Style size expectations account for `flexbox_balance`.
  Runtime features, dependency versions and cache algorithms are unchanged.
- In-flow root sizing keywords are resolved before final layout, preserving
  constraints, the other axis, box-sizing and percentage-height definiteness.
  Block automatic width stretch no longer overrides intrinsic width. Flex
  ContentSize ignores the measured container's own min/max until the caller
  applies them; final used main dimensions constrain greedy/balanced wrapping
  independently of descendant percentage definiteness. These are algorithm
  changes, not only publication instrumentation. Focused regressions and a
  small independent Chromium reference accompany the framework
  [sizing/query contract](../../docs/RETAINED_FLEX_SIZING_QUERIES.md).
- `Contain::INLINE_SIZE` / `SIZE` implement empty-content sizing in the declared
  axes through the common leaf box model, integrated with Flex, Grid and Block.
  Descendant layout remains normal; width-sensitive leaf measurement sees the
  contained used width before measuring the other axis. Layout/paint containment
  now claims both absolute and fixed positioning descendants. Clipping and
  stacking remain renderer responsibilities. Framework query boundaries choose
  layout containment explicitly, stronger than Chromium `container-type` alone.
  See [size containment](../../docs/RETAINED_SIZE_CONTAINMENT.md) for semantics,
  adapter coordinates, validation and limitations. Vertical writing and
  contain-intrinsic-size fallback are not implemented.

The framework now publishes independent first/last horizontal baselines and
implements the documented Flex item/self sharing subset. Vertical writing and
full CSS baseline conformance remain open. See the
[current contract and limits](../../docs/RETAINED_LAST_BASELINE.md).

## Verification

Owned custom/ConstraintBox final overflow now starts from actual child output,
discarding the temporary leaf sizing helper's desired-content extent and adding
scroll end padding once. Custom parent metrics accept finite signed content
baselines outside fixed host height; leaf intrinsic metrics remain separately
validated. See [contract](../../docs/RETAINED_OWNED_CONTAINER_OVERFLOW.md).

Explicit baseline-complete direct-child queries now reuse final container
arithmetic under scoped geometry/cache/observation/OOF write suppression.
Size-only queries retain their weaker baseline contract. No persistent cache or
per-node field is added; explicit queries can repeat considerable layout work.
See [contract and limits](../../docs/RETAINED_DRY_BASELINE_QUERY.md).

```powershell
mise exec -- cargo test --manifest-path vendor/taffy/Cargo.toml --offline --lib
mise exec -- cargo test --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib
```

Validation counts and exact toolchain for the current change are recorded in
the framework contract above. Historical pixel and performance experiments
predating these changes remain historical evidence; this baseline publication
change does not claim a measured speedup or complete Flex conformance.
