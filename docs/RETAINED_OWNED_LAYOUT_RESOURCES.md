# Owned UI layout resources

Status: experimental Rust protocol, 2026-11-18. Full RFC remains active.
The user's "99%" means common UI should feel fluid and intuitive; it is not a
numeric success ratio or required p99 gate. Timings diagnose and compare work.

## Why this boundary exists

The retained kernel already supports pure custom parents, direct-parent metadata
and constraint-dependent content measurement. The owned UiDocument input model
could only express Box/Fixed/Image/Text, so a frontend using that boundary could
not compose those existing layout capabilities. This stage exposes them through
the same immutable resource table and atomic source batch. It does not introduce
a second Flex solver, renderer or custom-parent algorithm.

## Public contract

Source schema5 advances to6; schemas1–5 explicitly reject. Constructors choose6.
Complete UiNode literals, exhaustive UiContent/UiOperation/UiError matches and
stored experimental snapshots need migration. This is not a frozen wire format.

| Source declaration | Installed host resource | Ordinary lowering |
| --- | --- | --- |
| UiContent::Measure(id) | UiResources::measure(id, ContentMeasureSnapshot) | MeasureSnapshot::Content |
| UiNode.parent_layout: Option<UiResourceId> | UiResources::parent_layout(id, ParentLayoutSnapshot) | Transaction::parent_layout |
| UiNode.parent_data: Option<UiResourceId> | UiResources::parent_data(id, ParentData) | Transaction::parent_data |

UiNode::new defaults both optional fields to None. The corresponding
UiOperation::ParentLayout/ParentData replace the optional reference; None clears
it. Content remains a complete typed replacement. These are normal batch writes,
coalesced against final source/effective state with styles, topology and content.

The host installs pure owned Debug + Send + Sync Rust snapshots. Source messages
contain IDs, without Taffy styles/handles, platform objects or mutable frontend
callbacks. UiResources and messages can cross threads; each receiving document
still has one local owner. Installed pure algorithms execute during layout and
dry queries, may execute repeatedly or be skipped by caches, and cannot treat
invocation as a business event. Private candidate computations can be discarded.
As with the underlying traits, purity is a caller contract, not a Rust guarantee
that interior mutability is impossible.

The existing [parent protocol](RETAINED_CUSTOM_PARENT_LAYOUT.md),
[independent allocation](RETAINED_ALLOCATION_STYLES.md),
[parent data](RETAINED_PARENT_DATA.md) and
[content measure](RETAINED_CONTENT_MEASURE.md) contracts remain authoritative:

- Measure content is leaf-only. It contributes natural size and horizontal
  first/last baselines, with normal constraints, percentage bases, sizing mode
  and axis-demand inputs. It adds no new paint primitive; backgrounds and
  decorations remain ordinary node properties.
- ParentLayout requires Box content and Flex/None display, for authored and
  effective styles. Grid/ConstraintBox or measured/text/image hosts reject;
  existing direct absolute/fixed-child restrictions remain. Clearing the
  snapshot restores ordinary Flex.
- ParentData is authored immutable metadata read by the direct parent, separate
  from CSS item styles. Ordinary Flex can ignore it. ParentLayoutSnapshot keeps
  its optional ParentMeasure, inline and full independent allocation phases;
  resource registration does not select or replace those phases.

## Resource identity, adoption and queries

All resource kinds share the existing host table. An ID binds one immutable
resource; same snapshot clones are accepted, a different handle/value or kind
under the same key returns ResourceConflict. Use new IDs for changed inputs.
Two IDs can alias the same snapshot. Such a source change can advance revision
without requesting a visual frame when ordinary property equality holds.

Missing referenced IDs return MissingResource. A valid ID used for a different
kind returns ResourceKindMismatch, including Image/Text references. Unsupported
Text on a build without retained-text still returns UnsupportedContent.
The document pins live referenced resources and stays bound to its original
table identity. The table itself keeps every binding until table retirement;
there is no unregister, eviction or byte budget.

Snapshot export stores resource IDs, not algorithms/data. The receiving host
must install the bindings; normal recovery reconstructs layout. New resource
references, field clears, content transitions and same-batch create/metadata
all stage in the ordinary private TreeEdit candidate. Resource/type, style,
measure, parent output, layout or complete-geometry failure adopts no partial
source/frame. Image/Text owners and explicit measurement clear through the
existing final-content path. Accepted replay does not execute the batch again.

UiDocument now exposes the existing isolated kernel queries with external IDs:

```rust,ignore
document.dry_measure_node(node, child_input)?;
document.dry_measure_node_axis(node, child_input, LayoutAxis::Vertical)?;
document.dry_measure_node_baselines(node, child_input)?;
```

They return ChildLayoutResult/axis size, validate identities and inputs, and use
the same layout protocols as normal layout. They do not alter source revision,
published node layout or the immutable frame. They retain the kernel's private
tree/query clone and layout costs; they are not a constant-time getter. Pure
resources may run during a query, so this is not a no-computation promise.

FramePacket/LocalFrame retain render output and paint resources needed for old
frames. They do not retain a pure parent algorithm or leaf measurer merely to
paint old geometry. Tests confirm those owners can retire while old frames
remain consumable, including real glyph resources.

## Costs, evidence and remaining direction

Each node adds two optional source references and can pin additional resource
handles. Resource validation scans live nodes/references. Source directories,
ordinary candidate cloning, complete geometry validation, resource-table
lifetime and isolated query costs remain. Custom algorithms can themselves be
expensive; this adapter does not bound their CPU time. No timing/FPS or memory
improvement is claimed.

[Verification and commands](performance/2026-11-18-owned-layout-resources/README.md)
compare custom-parent composition with separate ordinary complete Flex trees,
including wrap/resize, changing measurements/metadata, independent allocation,
real Noto text, dry queries, rollback and old-frame consumption. GPU tests compare
all output pixels against the ordinary full reference.

Still open: logical component integration with this source model, automatic
builder scheduling/output ownership, general content-feedback proofs and the
complete RenderBox contract; portable algorithm/resource encoding and JS/Go ABI;
resource eviction/budgets; independent native presentation and representative
desktop effects/CPU continuity observation. This stage closes an API access gap,
not the complete rendering/layout RFC.
