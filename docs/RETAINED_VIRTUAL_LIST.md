# Declared-extent vertical virtual lists

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
require a new metrics snapshot, or a fixed row height chosen by the application.

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

Use the full ordinary list for small datasets or as a correctness reference.
Remaining work includes measured/estimated row-height feedback, anchor retention,
incremental height edits, sticky/horizontal layouts and accessible all-data
navigation. Work remains on the ordinary candidate/source/producer paths; no
performance guarantee follows solely from bounded node count. See
[verification and reproduction](performance/2026-11-19-virtual-list/README.md).
