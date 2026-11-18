# Vertical virtual lists with declared or measured extents

Status: experimental Rust API,2026-11-19. Complete RFC stays active.
See [decision, alternatives and limits](ADR_RETAINED_VIRTUAL_LIST.md).

`retained::virtual_list` provides `VirtualList<C>`, `VirtualListProps<C>`,
`VirtualListItems<C>`, `VirtualListMetrics` and `VirtualListWindow`. It composes
ordinary keyed components and Flex; no renderer-specific row API is required.

```rust,ignore
let scroll = ui.state(parent, 0, || [0.0; 2])?;
let items = VirtualListItems::<Row>::new(
    VirtualListMetrics::fixed(100_000, 24.0)?,
    move |index| Ok((data[index].id, data[index].clone())),
);
let mut props = VirtualListProps::new(items, scroll, [width, height]);
props.overscan = 48.0; // logical pixels before and after the viewport
props.max_rows = 256; // rejects excessive work before row factories run
edit.components::<VirtualList<Row>>(parent, host, [(list_key, props)])?;
```

Row implements ordinary Component, with Clone Props. It mounts inside an exact
declared-height clipped wrapper, so text/images/nested Flex retain normal layout
and paint contracts. Internal spacing belongs inside the row. Equal dataset
handle plus ordinary source equality permits producer reuse. Replace the immutable
VirtualListItems handle when row data/order changes; cloning keeps identity.
Factories must be pure and are only called for selected indices. Stable keys
must represent row identity; the selected window rejects duplicate keys.

`VirtualListMetrics::variable(heights)` accepts positive finite declared row
heights and builds a shared prefix table. Query `window(offset,height,overscan,
max_rows)` for the selected range, effective offset/maximum, leading/trailing and
total. Fixed metrics need no per-row table. Empty data or zero viewport height
mounts no rows. Invalid dimensions return InvalidVirtualList; too many selected
rows return VirtualListLimit without partial UI adoption.

The viewport is an explicit logical content-box size. For responsive Flex, create
an ordinary host with proven content-independent assignment, use allocation_builder
or an Allocation producer to read known_size, then pass that size through Props.
The builder's complete final-input validation still applies. Window width is not
substituted for the host's allocation. Height changes after width/font updates
require a new metrics snapshot, a fixed row height chosen by the application,
or the optional mounted-row feedback below.

Scroll remains an ordinary stable Signal<[f32;2]> with vertical requested offset.
Changing its identity on an existing list rejects; remount with another key to
change source ownership. The list binds the effective clamped offset and follows
source changes through its Content producer in the complete owner transaction.
When data shrinks the requested source may exceed used range; applications may
explicitly normalize it to metrics.maximum(viewport_height). Pure producers never
write application source state as a hidden side effect.

Normal combined handle_input/process_input_effects invokes the list's source-owned
wheel handler. It consumes representable distance and leaves the remainder for
ancestor handlers/defaults using shared EventContext::scroll_wheel. The original
row can retire as the visible window changes; default scrolling resumes at the
surviving handling ancestor from the installed route. Foreign/stale/reparented
routes still reject/skip under the existing policy. A prevented default stops
unhandled default movement; consumed source movement is ordinary callback state.

For programmatic row reveal:

```rust,ignore
let next = metrics.reveal(row_index, current_requested_offset, viewport_height)?;
ui.transaction(|tx| tx.set(scroll, [0.0, next]))?;
// Host renders and installs the complete image, then adopts its input scene.
// The application can now focus the newly mounted row's current node identity.
```

Off-window rows unmount, cancel short-lived tasks and clean up effects. Overlap
and keyed reorder retain live local state; re-entry after retirement mounts fresh.
Keep drafts/selection in parent/business state by ID when they must survive.
Current Tab/focus traversal uses mounted rows; all-data keyboard policy is explicit
future work. Focus/capture retirement follows host installation/scene adoption.
No unbounded component/GPU cache or implicit KeepAlive is created.

## Preserve reading position on data changes

The [keyed anchoring follow-up](ADR_RETAINED_LIST_ANCHORING.md) adds an optional
immutable global key index and pure scroll policy. `VirtualListItems::keyed`
validates key count/uniqueness once; the payload factory returns Props, with no
key duplication. It still runs only for selected rows. `key(index)` reads the
index without constructing a row. Clones share metadata and factory identity.

```rust,ignore
let next = VirtualListItems::<Row>::keyed(
    VirtualListMetrics::variable(data.iter().map(|row| row.height))?,
    data.iter().map(|row| row.id),
    move |index| Ok(rows[index].clone()),
)?;
ui.transaction(|tx| {
    let result = tx.rebase_virtual_list(
        items_source, scroll, next, old_height, new_height,
        VirtualListScrollPolicy::KeepVisible,
    )?;
    // If viewport changes, write its source here too, in the same transaction.
    Ok(result)
})?;
```

Feed items_source to an ordinary ComponentProducer that supplies the list's Props.
This replaces data and offset together before computed/producer/Flex settlement.
EventContext exposes the same operation for data changes triggered by an input.
The helper reads the latest scroll source when invoked, so an earlier captured
background offset cannot undo a newer wheel. Failed adoption leaves both old data
and offset live. Pure `old_items.rebase(&next,...)` is available for other owners.

KeepVisible keeps the first intersecting key and pixel position, preferring a
following then preceding surviving old neighbor if removed. KeepOffset preserves
numerical position within new bounds. FollowEnd{tolerance} follows new end only
when previously near the old end; readers away from the end retain their anchor.
The result names offset/key/reason/clamped. Bounds, empty data and shortened rows
can prevent exact preservation. Anchor policies require keyed data; KeepOffset
also accepts unindexed data. Index build uses O(N) metadata and deleted-anchor
fallback may scan old keys, without invoking payload factories.

Use the full ordinary list for small datasets or as a correctness reference.
Remaining work includes incremental height indexing, sticky/horizontal layouts and accessible all-data
navigation. Work remains on the ordinary candidate/source/producer paths; no
performance guarantee follows solely from bounded node count. See
[verification and reproduction](performance/2026-11-19-virtual-list/README.md).

## Optional mounted-row automatic height feedback

Use `MeasuredVirtualList<C>` when rows contain ordinary natural-height Flex,
text or images. Its dataset source starts with estimates and must be writable
and keyed. The component reads this source itself; a separate parent
ComponentProducer is unnecessary unless the viewport or other Props are reactive.

```rust,ignore
let data = VirtualListItems::<Row>::keyed(
    VirtualListMetrics::fixed(rows.len(), 32.0)?,
    rows.iter().map(|row| row.id),
    move |index| Ok(data[index].clone()),
)?;
let items = ui.state(parent, 10, || data)?;
let scroll = ui.state(parent, 11, || [0.0; 2])?;
let mut props = MeasuredVirtualListProps::new(items, scroll, [width, height]);
props.overscan = 64.0;
edit.components::<MeasuredVirtualList<Row>>(parent, host, [(list_key, props)])?;

// Host preparation, after ordinary source/producer/layout settlement:
ui.local_frame(paint_viewport)?;
let report = ui.settle_virtual_list_measurements(2048)?;
if report.changed_rows > 0 {
    schedule_next_ordinary_turn(); // not a loop inside this prepare/input call
}
let frame = ui.published_local_frame().unwrap().clone();
// Build/install the complete CPU/GPU image, then adopt its matching input scene.
```

One call measures mounted rows at committed width with ordinary content sizing,
then adopts new metrics and scroll compensation in one source transaction.
`VirtualListMeasureReport` reports measured_rows, changed_rows and optional commit.
KeepVisible is default; FollowEnd only follows a reader already near the end.
Empty content floors at minimum_height (default1px). Hidden/zero-sized lists
retain estimates. Shared feedback writers for either items or scroll reject;
both source identities stay fixed for the mounted instance.

The max_rows argument is a global query budget, separate from each list's mount
limit. If any complete mounted list exceeds the remaining budget, the call fails
without partial source adoption. Same values do not create another transaction;
stable render/source revisions skip repeated queries. Updated content and widths
remeasure mounted rows; offscreen rows retain estimates until mounted. Natural
row height must not depend on the previous estimate as a feedback algorithm.

`items.with_heights([(index,height),...])` is also available for explicit metric
edits and reuses keys/factory. It rebuilds one prefix table on actual changes;
it is not yet an incremental height tree. Dry layout still copies the live Taffy
tree once per list. See [decision and costs](ADR_RETAINED_LIST_MEASUREMENT.md) and
[complete-image validation](performance/2026-11-19-list-measurement/README.md).
