# Retained rendering implementation progress

The active implementation goal covers the complete runtime requested on
2026-11-11. Work continues in `codex/rendering-performance-research`.

Current user direction (2026-11-18): "99%" means common UI should feel fluid and
intuitive; it is not a numeric success ratio or a required p99 gate. Timings,
tail samples and cadence runs remain diagnostic evidence for comparisons and
regressions. Earlier p99/percentage acceptance wording below is historical.
Prioritize missing public UI/Layout capabilities and ordinary Flex behavior;
rare combinations and scene-specific tuning do not take priority over them.

The [scoped event stage](RETAINED_SCOPED_EVENTS.md) adds EventHandler/EventContext
and keyed registration/owned-node UiInteraction policy. LogicalUi publishes the
existing UiInputScene over complete current frame geometry; UiInputRouter retains
shared pointer/focus/Enter/Space behavior. Dispatch rechecks current generation,
route/eligibility/handler identity, ignores retired callbacks and uses ordinary
source/component/layout transactions, explicitly settling a complete local frame
at the current published constraints even without persistent producers. Event
after_commit actions run only after successful adoption, bridging Provider
Command→Store→UI without sending business
work from a failed candidate. Sparse handler/policy registry adopts/retires with
scopes/output, in transactional and manual paths. No solver/GPU/schema change;
Error adds InvalidEventHandler. See
[verification](performance/2026-11-18-scoped-events/README.md).
Full-scene map construction/per-event validation, linear handler lookup and normal
source/producer/candidate costs remain, with no timing/FPS claim. Automatic style/
wheel/reveal feedback application, task/native host/IME/semantics integration,
complete source unification, hooks/macros/KeepAlive and budgets stay active.
Final retained691passed/1ignored, text/WGPU725/63 and hello78/29;
10compiler doctests pass in both core configurations. No-default16passed/2docs.
Explicit scoped-events GPU compares3complete images and Noto3 on
GTX1650Ti/Vulkan/571.96. Final library Clippy -D warnings, complete retained-gpu
harness, scoped format/diff,308local links and5WIP hashes pass. Final code covered;
no native displayed latency/cadence or performance improvement inferred.

The [scoped provider stage](RETAINED_SCOPED_PROVIDERS.md) adds explicit StoreKey
and scope-local Store-input/typed Command declarations with nearest logical-parent
lookup. Consumers cache the first identity or optional absence; local overrides
apply to new consumers, same identity redeclaration is idempotent, replacement
requires remount. Context/LogicalEdit supply explicit use/try_use; missing required
dependency reports logical key path and Rust type name. Sparse Rc registry adopts
with ordinary candidate publication, retires by scope generation and rejects live
aliases to retiring Store sources in both transactional/manual paths. Store
providers share captured input rather than resampling business state. No per-node
field/source schema/solver/renderer change; Error adds provider diagnostics. See
[verification](performance/2026-11-18-scoped-providers/README.md).
Registry copying/lifetime scans and existing source/producer/candidate/layout work
remain. No FPS/memory claim. Hooks/macros, event/task lifecycle, KeepAlive, wire
keys/schema, budgets and independent native presentation remain; full RFC active.
No-default16passed/2compile-fail docs, retained683/1ignored, text/WGPU717/62 and
hello77/28;9compile-fail docs pass in both core configurations. Explicit
provider→Store→UI GPU compares3complete images and Noto3 on GTX1650Ti/Vulkan/571.96.
Library Clippy -D warnings, complete retained-gpu harness, scoped format/diff,
314local links and5WIP hashes pass. Final code covered; no native cadence inferred.

The [typed Command stage](RETAINED_TYPED_COMMANDS.md) adds backend-independent
Command/channel/Sender/Receiver/Ticket/Request with queue-qualified IDs, bounded
queued+running admission, FIFO dispatch, owned reliable results and optional
business StoreRevision. Busy/Closed returns the original request. Queued cancel
removes work/releases capacity; running cancel is an explicit worker request,
with success/business-error/cancel/abandoned/closed outcomes distinct. Ticket drop
does not cancel business work. Executor futures integrate without a new runtime.
Command→Store→selector→ComponentPlan→Flex controls preserve the business committed
vs UI applied boundary. See
[verification](performance/2026-11-18-typed-commands/README.md).
Provider/event/task integration, per-object execution policy, wire schema and byte
budgets remain explicit follow-up; FIFO dispatch alone is not serial completion.
No solver/renderer/schema change or FPS claim. Complete RFC stays active.
No-default16passed/2compile-fail docs, retained673/1ignored, text/WGPU707/61 and
hello76/27;8compile-fail docs pass in both core configurations. Explicit
Command→Store→UI GPU compares3complete images and Noto3 on GTX1650Ti/Vulkan/571.96.
Library Clippy -D warnings, complete retained-gpu harness, scoped format/diff,
305local links and5WIP hashes pass. Final code covered; no native cadence inferred.

The [business Store stage](RETAINED_BUSINESS_STORE.md) adds backend-independent
StoreWriter/ReadStore/immutable snapshots and versioned latest-whole-state
subscriptions. Unique business writer commits expected revisions; equal state
does not publish/wake. UI attach_store/apply_store bridges one captured snapshot
through the existing source helper/transaction; StoreSelector reuses computed
equality and ordinary bindings/component observations. Failed UI adoption keeps
the old frame and does not acknowledge; a newer business revision remains pending.
Scope-owned cancellation removes visual observation on successful retirement,
leaving the application Store alive. No second graph or renderer/solver change;
source schema6 remains, Error adds InvalidStore. See
[verification](performance/2026-11-18-business-store/README.md).
Command/tickets/provider resolution, persistent collections, coordinated inputs,
wire schema, hooks/macros/KeepAlive, semantic input, budgets and independent
presentation stay active. Whole-state equality/Arc retention, subscription wake
scans and usual source/producer/candidate/layout costs remain; no FPS claim.
Final no-default5passed/1compile-fail doc, retained661/1ignored, text/WGPU695/60
and hello75/26;7compile-fail docs pass in both core configurations. Explicit
Store bridge GPU compares3complete images and Noto3 on GTX1650Ti/Vulkan/571.96.
Library Clippy -D warnings, complete retained-gpu harness, scoped format/diff,
299local links and5WIP hashes pass. Final code covered; no native cadence inferred.

The [component selection plan stage](RETAINED_COMPONENT_PLANS.md) adds owned
ComponentPlan plus read-only ComponentInputs and automatic ComponentProducer.
Explicit Rust branch/show/match/list selection uses ordinary Content source
observations/final-value scheduling. All declared regions reconcile first, then
share one heterogeneous props checkpoint, then mount fresh instances; even the
first new mount sees all planned final inputs. Same arm/type/host keeps state,
new identity unmounts, hidden show skips its factory, errors preserve old frame.
Original single-list components share Prepared; no persistent field/schema or
solver/renderer change. Existing one-host writer/DAG/final validation remains.
See [verification](performance/2026-11-18-component-plans/README.md).
View macros/generated arm keys, hooks/KeepAlive, providers/Store/Command,
source unification, event/semantics integration, general output ownership,
complete RenderBox, budgets and independent presentation stay active. Per-plan
dispatch boxes and usual reconciliation/source scans/order/candidate costs remain;
no FPS/memory claim. The complete RFC remains active.
Final retained652passed/1ignored, text/WGPU686/59 and hello74/25;5compile-fail
docs pass in both core configurations. Explicit plan GPU compares8complete
images and Noto3 on GTX1650Ti/Vulkan/571.96. Library Clippy -D warnings, complete
retained-gpu harness, scoped format/diff,301local links and5WIP hashes pass.
All final production code is covered; counts overlap, no native cadence inferred.

The [independent component region stage](RETAINED_COMPONENT_SITES.md) adds
ComponentSite/component_sites/components_at. Parent declarations own region
placement, region declarations own their keyed contents; same item keys across
regions are independent. Reordering regions keeps instances; selecting new
component types/arm keys retires old state, empty selection keeps site, site
removal retires descendants/producers. No layout/render wrapper is added. The
existing optional identity record distinguishes components and regions; schema6,
kernel/renderer and ordinary final candidate stay. Context validates direct-site
ownership; caught errors poison the outer edit. Props still settle per content
call and existing host-writer producer ordering remains. See
[verification](performance/2026-11-18-component-sites/README.md).
Automatic Branch/Match factories, cross-site heterogeneous input batches,
KeepAlive/hooks/macros, Store/Command/source unification, output ownership,
full RenderBox, resource budgets and independent native presentation remain.
Region scopes/maps/sets, source scans, full ordering/candidate/geometry costs
remain; no FPS/memory benefit is claimed. Complete RFC stays active.
Final retained643passed/1ignored, text/WGPU677/58 and hello73/24;4compile-fail
docs pass in both core configurations. Explicit region GPU compares10complete
images and Noto3 on GTX1650Ti/Vulkan/571.96. Library Clippy -D warnings, complete
retained-gpu harness, scoped format/diff,298local links and all5WIP hashes pass.
Tests cover final code; counts overlap. No native cadence or fluency is inferred.

The [typed component stage](RETAINED_TYPED_COMPONENTS.md) adds Component with
aggregate ReadProp inputs, explicit ComponentContext and LogicalEdit::components
keyed declarations. Same key/type/host retains initialization/state/bindings/
outputs; new key/type/host mounts fresh, with zero/multiple outputs supported.
The same logical reconciliation/props/producer/source/frame boundary applies;
errors/unwind poison the outer edit and final failure preserves old instances.
Scope gains optional boxed type/host identity; source schema6/kernel/renderer
remain. Error adds InvalidComponent. See
[verification](performance/2026-11-18-typed-components/README.md).
Per-site maps/sets/rows, props/derived scans, full logical ordering and ordinary
candidate/geometry costs remain. Independent/heterogeneous sites, Branch/Match,
KeepAlive/hooks/macros, Store/Command, source unification, output ownership,
full RenderBox, budgets and independent presentation stay active. No FPS benefit
is claimed; the complete RFC is not complete.
Final retained636passed/1ignored, text/WGPU670/57 and hello72/23 pass, including
7new core and real-Noto CPU;4compile-fail docs pass in both core configurations.
Explicit keyed-component GPU compares6complete images and Noto GPU3 on
GTX1650Ti/Vulkan/571.96. Library Clippy -D warnings, complete retained-gpu harness,
scoped format/diff,294local links and all5WIP hashes pass. Full suites/hardware
cover final production code; counts overlap and no visible cadence is inferred.

The [atomic prop batch stage](RETAINED_PROP_BATCHES.md) adds LogicalEdit::props
with restricted PropBatch::prop declarations. Related heterogeneous final values
settle once through the same source helper; temporary tuples cannot be read or
measured, and restored roots skip derived work. Failed declarations poison their
batch; fallible/unwinding settlement rolls back batch-only source allocations,
maps and counters while consuming generations. Outer final validation remains.
No persistent source/schema/kernel/GPU format change; Error adds InvalidPropBatch.
See [verification](performance/2026-11-18-prop-batches/README.md).
Per-batch copying/whole derived scans, scratch allocations and ordinary candidate/
frame costs remain; no timing/FPS benefit is claimed. Typed component sites/
contexts/macros, Store/Command, source integration, output ownership, full
RenderBox, resource budgets and independent presentation remain active.
Final retained629passed/1ignored, text/WGPU663/56 and hello71/22 pass, including
7new core and real-Noto CPU;3compile-fail docs pass in both core configurations.
Explicit keyed-input GPU compares6complete images and Noto GPU3 on
GTX1650Ti/Vulkan/571.96. Library Clippy -D warnings, complete retained-gpu harness,
scoped format/diff,288local links and all5WIP hashes pass. Full suites/hardware
cover final production code; counts overlap and no visible cadence is inferred.

The [ReadProp stage](RETAINED_READ_PROPS.md) adds owner-local live read-only
component inputs. Keyed LogicalEdit::prop replacements reuse the ordinary
responsive source staging/derived graph; read_prop/bind_prop integrate producer
observations, computed/effect inputs and typed sinks. Same-key payload changes
retain mount/state/outputs; scope/generation/namespace checks and final atomic
source/layout/frame adoption remain. No Signal setter/thread capability or new
source schema/kernel/renderer algorithm is added.
See [verification](performance/2026-11-18-read-props/README.md).
Prepared-map copying, repeated complete derived scans and ordinary plan/candidate/
geometry costs remain. Component sites/contexts/macros, batch input replacement,
Store/Command, source integration, output ownership, full RenderBox, budgets and
independent presentation stay open; no timing/FPS benefit is claimed.
Final retained622passed/1ignored, text/WGPU656/55 and hello70/21 pass, including
8new core and real-Noto CPU;2ReadProp compile-fail docs pass in both core feature
configurations. Explicit keyed-payload GPU passes6complete images and Noto GPU3
on GTX1650Ti/Vulkan/571.96. All suites/hardware cover final production behavior;
counts overlap and no visible-cadence qualification is inferred.
Library Clippy -D warnings, complete retained-gpu harness, scoped format/diff,
286local links and all5unrelated WIP hashes pass.

The [source-driven Content stage](RETAINED_CONTENT_PRODUCERS.md) adds Content
kind/input to the existing build plan and persistent producer protocol. Ordinary
data-dependent keyed structure/text can construct without first observing a
layout slot, so automatic/intrinsic Flex/Grid parents need no allocation proof.
The same ancestry/explicit/inferred measurement graph, dynamic descendant tasks,
source observations/reverse index and atomic scope/effect/frame boundary apply.
Dimension-reading kinds keep their proofs. Rust exhaustive enum matches need
the new variant; source schema6 and vendor/renderer algorithms are unchanged.
See [verification](performance/2026-11-18-content-producers/README.md).
Per-task planning previews/DAG, candidate copies, coarse invalidation and full
geometry costs remain. ReadProp/Branch/Match/KeepAlive/macros, Store/Command,
component/source integration, output ownership, full RenderBox, budgets and
independent native presentation remain active. No timing/FPS benefit is claimed.
Final retained614passed/1ignored, text/WGPU648/54 and hello69/20 pass, including
8new core cases and real-Noto CPU. Explicit keyed-list GPU compares6complete
images and real-Noto GPU3 on GTX1650Ti/Vulkan/571.96. Library Clippy -D warnings,
complete retained-gpu harness, scoped format/diff,293local links and all5WIP
hashes pass. Complete suites and keyed-list GPU were rerun after the resize guard;
real-Noto GPU/Clippy/harness also include that final production correction.

The [full Grid/Flex allocation stage](RETAINED_FULL_GRID_ALLOCATION.md) extends
the same full builder/Allocation task/persistent producer protocol across mixed
ordinary Grid and Flex ancestry. Both Grid axes need independent numeric/fr
tracks and explicit implicit patterns; path items have independent preferred
sizes or supported stretch. The kernel supplies actual area bases, known border
boxes, soft space and separate hard bounds; no alternate allocator is added.
Complete parent styles, ordered child IDs and effective item styles validate
before atomic adoption. Independent Grid sibling content stays free; measured
Flex titles inside cells keep inferred title-before-body scheduling. Shared
inline helpers preserve their earlier scope. Source schema6 is unchanged.
See [verification](performance/2026-11-18-full-grid-allocation/README.md).
Track/ancestry/child scans, private previews/candidates and full geometry costs
remain. Intrinsic/automatic feedback, full RenderBox, callback output ownership,
component integration, budgets and independent presentation stay open. No FPS,
latency or memory benefit is claimed; the complete RFC remains active.
Final retained606passed/1ignored, text/WGPU640/53 and hello68/19 pass, including
7new core cases and real-Noto CPU. Explicit numeric Grid/Flex GPU passes12complete
images; real-Noto GPU passes3 on GTX1650Ti/Vulkan/571.96. Library Clippy -D warnings,
complete retained-gpu harness, scoped format/diff,288local links and all5WIP
hashes pass. Counts overlap; all full suites cover the final production code.

The [inferred measurement task stage](RETAINED_INFERRED_MEASUREMENT_TASKS.md)
connects existing measured-sibling proofs to the normal once-only/persistent
planner. Queued hosts in the measured sibling subtree precede a dependent full
Allocation task, independent of registration/ID order; dynamic descendant tasks
join the next graph. Ancestry/explicit/measurement edges share limits and
deduplication. Cycles and retroactive predecessors reject without replay.
Final callback-free order validation re-derives this graph in addition to normal
complete builder input/resource/source/frame validation. The original independent
parent/containment domain keeps precedence and source schema6 is unchanged.
See [verification](performance/2026-11-18-inferred-measurement-tasks/README.md).
Repeated proof/sibling scanning, graph construction and private previews/candidates
remain; arbitrary nonlocal writes still need after, and general writer ownership,
feedback/RenderBox, component integration and independent presentation stay open.
No timing/FPS gain is claimed; the complete RFC remains active.
Final retained599passed/1ignored, text/WGPU633/52 and hello67/18 pass. Logical
filter112passed/10ignored includes the5new core cases, existing producer/keyed
regressions and updated title-before-body case. Explicit numeric/Noto GPU pass
6/3complete images on GTX1650Ti/Vulkan/571.96. Library Clippy -D warnings,
complete retained-gpu harness, scoped format/diff,285local links and all5WIP
hashes pass. Counts overlap; full runs cover the final shared graph validation.

The [measured Flex sibling stage](RETAINED_MEASURED_FLEX_SIBLINGS.md) lets ordinary
full-axis allocation builders compose an automatic content-sized title/toolbar/
sidebar with a remaining-space body. Only the path child's contribution must
remain independent; content-dependent sibling subtrees record complete layout
style/structure/measure/algorithm/metadata inputs. Final dependencies plus actual
full input validate before atomic source/keyed/frame adoption. Late same-number
measurement changes reject; same-paragraph text color or same-intrinsic image
pixel changes keep the separate paint channel. Ordered sibling producers work
through existing explicit prerequisites; wrong order rejects without replay.
No new public type/schema/vendor algorithm. See
[verification](performance/2026-11-18-measured-flex-siblings/README.md).
Sibling subtree scanning/retained observations and ordinary previews/candidates
remain; automatic output dependency discovery, content feedback on the host
path, full RenderBox and independent presentation stay open. No FPS/latency gain
is claimed; the complete RFC continues active.
Complete retained594passed/1ignored, final text/WGPU628/51 and hello66/17 pass;
new6core and real-Noto CPU are included. Explicit GPU compares12four-direction
sibling images and3real-Noto title images on GTX1650Ti/Vulkan/571.96. Final
library Clippy -D warnings, complete retained-gpu harness, scoped format/diff,
276local links and all5unrelated WIP hashes pass. Retained full preceded the
last layout_order observation; final text/WGPU/hello/GPU cover it. Counts overlap.
Final retained-only sibling filter6passed also covers the final order observation.

The [ordinary full-axis Flex allocation stage](RETAINED_FULL_FLEX_ALLOCATION.md)
extends the existing allocation_builder/Allocation tasks to content-independent
Row/Column chains, without inserting containment or replacing Flex layout.
Numeric main bases/explicit minima and externally sized root axes exclude
intrinsic feedback; NoWrap allows cross stretch and wrapped lines require
numeric cross sizes without baseline groups. Actual incoming full inputs use
the existing opt-in kernel records/exact cache keys. Final ancestor/order/
effective sibling-style proofs and complete inputs validate before atomic
source/keyed/frame adoption. The original independent-parent/containment proof
keeps precedence and its prior domain. Source schema6 is unchanged.
See [verification](performance/2026-11-18-full-flex-allocation/README.md).
Private previews/candidates, ancestor/sibling scanning and final geometry costs
remain; general intrinsic/ratio/cross-line feedback, full RenderBox, component
integration and independent presentation remain open. No timing/FPS gain is
claimed; the complete RFC goal continues active.
Final retained588passed/1ignored, text/WGPU622/50 and hello65/16 pass. The
builder filter45passed/5ignored includes all5new core cases and original
independent-parent/containment regressions. Explicit GPU controls pass24complete
numeric-pane comparisons and3real-Noto images on GTX1650Ti/Vulkan/571.96.
Library Clippy -D warnings, complete retained-gpu harness check, scoped format/
diff,282local links and all5unrelated WIP hashes pass. Counts overlap.

The [owned layout-resource stage](RETAINED_OWNED_LAYOUT_RESOURCES.md) closes a
public input gap: UiDocument can now compose existing pure custom parents,
direct-parent metadata and constraint-dependent content measurement, alongside
ordinary Flex and prepared text. Schema6 adds Measure content, optional parent
resource fields and corresponding operations; schemas1–5 reject. Resources
remain host-installed immutable handles, while source messages carry IDs.
The ordinary final-state TreeEdit candidate validates kinds, effective styles,
leaf constraints, fallible algorithms and complete geometry before adoption.
Document dry size/axis/baseline queries reuse isolated kernel queries without
publishing temporary layout. No new layout solver, paint primitive or renderer
is introduced. See [contract and remaining scope](RETAINED_OWNED_LAYOUT_RESOURCES.md)
and [verification](performance/2026-11-18-owned-layout-resources/README.md).
Resource scans/table lifetime, source/candidate cloning and private query costs
remain. This is framework/API integration, with no timing/FPS gain claimed.
Component/source integration, general content-feedback proofs/full RenderBox,
foreign resource encoding and independent native presentation remain open;
the complete RFC goal stays active.
Final complete retained583passed/1ignored, text/WGPU617/49 and hello64/15 pass.
Seven new regular core cases and real-Noto CPU are included in these suites;
both explicit GPU controls pass3submissions/3complete-image comparisons each.
Final library Clippy -D warnings, complete retained-gpu harness check, scoped
format/diff,300local documentation links and all5unrelated WIP hashes pass.
Counts overlap; ignored cases count as GPU evidence only when explicitly run.

The [focus reveal stage](RETAINED_FOCUS_REVEAL.md) adds schema5 ScrollIntoView
post-layout intents. Tab/explicit Focus carry adopted target/route through the
existing input planner; final layout/style changes settle before nearest signed
scrolling of unowned Hidden/Scroll ancestors. Logical geometry avoids projected
empty-clip traps. Ordinary private candidate checks publish resulting authored/
effective offsets and complete frame atomically, with merged unique route counts.
Pointer focus does not scroll; replay/visible repeat, fixed clips, extents and
style ownership retain explicit behavior. Tests cover nested/two-axis/oversized,
final style/wrap/create/reparent/rollback, real GPUI Tab and Noto CPU/GPU numeric
references. See [evidence](performance/2026-11-18-focus-reveal/README.md).
Per-intent paths/commits/frame validation and ordinary full candidates remain;
no timing/FPS gain or native p99 fluency acceptance is claimed. Full RFC active.
Final complete retained576passed/1ignored, text/WGPU610/48 and hello63/14 pass;
reveal专项9/1ignored, actual GPUI Tab and Noto CPU pass. Explicit final core/Noto
GPU compare2/3complete images. Library Clippy, complete retained-gpu harness
check, scoped format/diff/279local links and all5WIP hashes pass. Counts overlap;
final suites and GPU reruns include unique diagnostics/empty-request follow-up.

The [automatic overflow viewport stage](RETAINED_OVERFLOW_VIEWPORTS.md) derives
axis-selective padding/gutter clips from actual layout. Own image/glyph content
and descendants clip; host background/border/shadows keep the ordinary clip.
Full/selective/local/snapped publication, hits, damage and CPU/WGPU content share
that geometry. Viewport resize/policy/RTL changes invalidate cached descendant
clips. Shared GPU nodes grow96→112bytes; benchmark byte assertions use actual ABI
size and historical timing is not reinterpreted. No new layer/texture or source
schema. Numeric independent CPU/GPU controls cover all4owner policies, own real
Noto and70-level image/shadow ancestry. Ordinary wheel fixtures no longer author
manual viewport clips. See [evidence](performance/2026-11-18-overflow-viewports/README.md).
Native p99, CPU sustained complete frames, independent scrolling, rounded clips
and full RFC acceptance remain active; no new FPS/latency win is claimed.
Complete retained566passed/1ignored, text/WGPU600/46 and hello61/13 pass;
final overflow filter8passed/4ignored includes custom-parent cases, and all46
explicit framework hardware cases pass. New viewport/mixed-axis/deep-image GPU
controls compare24/9/4complete images; own-Noto CPU/GPU5images and prior wheel
CPU/GPU without manual source clips pass. Actual backend index rustc harness
passes9cases. Complete-suite/test-only follow-up ordering and costs are recorded
in the evidence; counts overlap and this does not qualify native fluency.
Final library Clippy, complete retained-gpu performance harness check, scoped
format/diff/272local links and all5unrelated WIP hashes pass.

The [wheel scrolling stage](RETAINED_WHEEL_SCROLLING.md) adds logical-pixel Wheel
input, adopted target/path records, immutable published scroll extents and
current-offset ancestor chaining. plan_input_scroll builds one exact-base normal
batch with narrow Scroll operations plus style feedback; events accumulate
without predicting installed hit geometry. Axes clamp/chain independently,
style-owned/Hidden axes yield remainder, and stale ancestry/recovery/disabled
targets reject. Schema4 adds Scroll and rejects1/2/3. Seven core cases, actual
GPUI ScrollEvent and real-Noto CPU/GPU controls exercise independent full
reference pixels and source/frame rollback. See
[evidence](performance/2026-11-18-wheel-scrolling/README.md). Scroll-map scanning/
rebuild, linear hit/paths and ordinary owner candidates remain; no wall-time/FPS
win is claimed. Latching/inertia/overscroll/cancelable wheel handlers,
independent compositor scroll and native input/presentation p99 remain open.
Full RFC stays active.
Verified complete retained561passed/1ignored, text/WGPU594/44 and hello60/12;
final7wheel core cases pass. Actual GPUI ScrollEvent integration and real-Noto
CPU pass; explicit nested-chain and real-Noto wheel GPU each pass4submissions/
4complete-image comparisons. Existing persistent Noto GPU passes5comparisons
(4submits/1skip). Final framework library Clippy, scoped format/diff/262local
links and five unrelated WIP hashes pass. Counts overlap. Production changes
preceded full suites; a later test-only stationary-hit assertion is covered by
the final7core cases and real-Noto wheel CPU/GPU reruns. These checks establish
correctness, not representative native common-interaction p99 fluency.

The [indexed producer invalidation stage](RETAINED_PRODUCER_INVALIDATION.md)
adds generation-safe source-to-producer reverse reads, atomically rebuilt with
accepted observations. Final changed source identities select equality-check
candidates. With unchanged registry/rules/logical tree/constraints/viewport and
a strict internal no-render-write/no-binding-sync certificate, the entire prior
accepted plan reuses without builder previews/input tracking. Complete final
frame geometry/source/lifecycle publication remains. Any related/uncertain
change retains the full producer plan because output writes are not host-local.
Six new core cases and real-Noto controls cover skips, computed/restored values,
conditional index rollback, bindings, responsive output, explicit edits, resize
retirement/generation and convenience/internal entry invalidation. See
[evidence](performance/2026-11-18-producer-invalidation/README.md).
Index rebuild/membership memory, full candidates/geometry and related-update
complete-plan costs remain; full RenderBox and native common-interaction p99
acceptance remain open. The complete RFC is active.
Four final alternating release pairs on41/161-node ordinary Flex with8/32producers
reduce unread/equal transaction p50 by93.0–98.8%, with p50/p99 wins in all4pairs.
Related8producer p50/p99 regress26.9%/27.5%; related32p50 slightly improves but
p99 worsens8.9%. First/final candidates retain9,600samples and exact patches/hashes;
this does not qualify related tails/native120Hz. Shared immutable source-read maps
reduce observation clone work; full-plan and index-rebuild costs remain explicit.
Final15producer cases, retained554/1ignored and library Clippy pass; text/WGPU
586/43 and hello58/11 passed before final convenience/internal entry fixes.
Those fixes are covered by final focused/retained regressions. Explicit Noto
GPU5images(4submits/1skip) and ephemeral Flex4images(3submits/1skip) pass; scoped
format/diff/259local links and sample/hash checks pass.

The [persistent layout producer stage](RETAINED_LAYOUT_PRODUCERS.md) registers
owner-local reusable callbacks by scope/key, automatically schedules them after
ordinary source transactions/background drains and frame changes, and retires
pending child registrations with keyed scopes. Tracked dynamic Signal reads,
original builder inputs, callback identity and conservative render/logical
invalidation govern content-callback reuse. Existing builders still run their
proof/measurement/final observations even when content callbacks reuse. Registry,
source observations, stats, frame session and complete output adopt atomically;
late source changes, stale cross-scope dependencies and cycles reject. Nine core
cases and real-Noto CPU/GPU controls cover automatic changes, conditional reads,
computed equality, dynamic children, all input kinds and rollback. See
[verification](performance/2026-11-18-layout-producers/README.md). Full registry
scans/clones, source-value retention, repeated DAG/preview/proof/final-layout work
remain. This is framework capability work without timing/FPS improvement claim;
arbitrary writer discovery, fine output-domain invalidation, full RenderBox and
native common-interaction p99 remain open. The full RFC is active.
Verified retained548passed/1ignored, text/WGPU581/43 and hello58/11; persistent
real-Noto GPU5full-image comparisons use4submits/1skip, prior ephemeral Flex4
comparisons use3submits/1skip. Final framework library Clippy and scoped
format/diff/268local-link checks pass. Counts overlap; only explicitly executed
ignored cases establish hardware evidence.

The [local revalidation follow-up](performance/2026-11-18-local-revalidation/README.md)
preserves the exact LocalFrame after checked layout when source revision,
pixel policy and selective paint/property storage identities remain unchanged.
This removes duplicate local topology construction/submission for equal output;
requested builders, previews, layout and final input/proof/geometry checks still
run. New revisions, real kernel geometry changes and sparse-local changes over
an older resolved baseline retain ordinary publication. Three new regular cases
check these boundaries and atomic source/frame rollback. The once-only-plan GPU
control now expects3submits/1skip across4complete-image comparisons. The full RFC
and native common-interaction p99 remain active acceptance work.
Complete retained539/1ignored, text/WGPU572/42 and hello57/10 pass; final real-Noto
extension passes5full-image comparisons with4submits/1skip. Library Clippy and
scoped checks pass. Four alternating release pairs show no stable owner CPU
gain (unchanged-plan p50 regresses0.7%/1.0% at67/515nodes); all3,200samples remain.
An isolated debug GPU consumer control avoids duplicate work: median of four
round p50s1.4655→0.00865ms, p99s2.0664→0.0108ms,400submits→400skips with8full-image
comparisons. Its800samples are distinct from release owner and native display
timing, and cannot establish whole-application speedup or fluency acceptance.

The [once-only layout build plan](RETAINED_LAYOUT_BUILD_PLAN.md) adds a
transaction-scoped scheduler for all four existing builder kinds. Current render
ancestry orders parents before descendants, explicit prerequisites order sibling
work, and append-only callback queues accept newly created keyed child hosts.
Callbacks can borrow transaction-local data and execute once outside intrinsic
measurement. Complete queued-graph checks and final input/proof/order observations
reject cycles, retroactive dependencies and late reparenting before atomic
source/lifecycle/layout/frame adoption; caught plan failures still reject the edit.
Nine regular core cases, one real-Noto case and two explicit GPU controls cover
ordinary nested Flex, responsive rules, all input kinds, keyed state/effects and
rollback. See [verification](performance/2026-11-18-layout-build-plan/README.md).
Complete text/WGPU569passed/42ignored and hello57/10 pass; retained-only535/1
preceded the final ninth test-only addition, then all9focused cases passed.
Both explicit GTX1650Ti/Vulkan GPU controls pass4complete-image submissions,
along with final framework library Clippy and scoped format/diff/link checks.
At that stage repeated inline input tracking refreshed layout/local snapshots,
including equal-width GPU submissions; the follow-up above now reuses certified
unchanged output. Unchanged callback/plan skipping is still not implemented.
Repeated draft previews and full graph checks remain substantial costs; this is
capability work with no timing or FPS gain claim. Persistent registrations,
automatic source read/write dependency discovery, unchanged-callback skipping,
general content feedback, full RenderBox and native common-interaction p99 remain
open. The full RFC is active.

The [adopted-frame input stage](RETAINED_UI_INPUT_ROUTING.md) adds backend-neutral
interaction policy, handler IDs, primary capture/hover/press, focus/Tab and Enter/
Space activation over an explicitly adopted immutable UiInputScene. Geometry,
clip/scroll/order and policy travel together; delayed dispatch validates current
runtime/epoch/tree/generation/path/handler. Disable/unmount/recovery cancel input
state; feedback joins ordinary StyleState batches, preserving caller fields.
Schema3 adds interaction and boxes large Create/Layout payloads. Ten core cases,
one explicit GPU case and one actual GPUI mouse-dispatch integration case were
added. Complete suites pass retained527/1ignored, text/WGPU560/41, hello56/9;
final input10cases and GPU3submits/2same-frame skips compare5complete images.
Actual Noto GPU5complete-pixel submissions and final framework library Clippy
also pass; scoped format/diff/document links pass.
See [evidence](performance/2026-11-18-ui-input-routing/README.md). Linear hit/path
allocation, input graph memory/rebuild/detection and normal feedback publication
costs remain; no timing/FPS improvement is claimed. OS capture/focus, wheel/
gestures/IME/accessibility, event acknowledgement and native sustained input p99
remain open along with full RenderBox/persistent structural scheduling/full RFC.

The [incremental style planning stage](RETAINED_INCREMENTAL_STYLE_PLANNING.md)
shares immutable authored/effective UI records, caches selected StylePlans and
tracks only winning-rule state/environment dependencies. StyleRef work bounds
cache once; shared identities skip equal comparisons. Attribute-only batches
reuse accepted source topology; structural batches retain complete validation,
and all publications retain ordinary retained/layout/geometry checks. Seven
new core cases cover dependency/index/record lifetime, base/metadata changes,
rollback, replay/recovery and sparse/dense merges. Verified: retained517passed/
1ignored; text/WGPU550/40; hello55/9; explicit GTX1650Ti/Vulkan style4 and glyph5
complete-pixel comparisons; framework library Clippy and scoped format/link/diff
checks pass. Four alternating release pairs on2,177-node ordinary Flex/Grid
reduce sparse/no-op apply p50 aggregates59.5–71.6%; dense p50 Flex14.482→13.945ms,
Grid15.436→13.643ms. Flex dense p99 still loses in2/4pairs; initial candidates
with dense regressions are preserved. See [raw evidence and scope](performance/2026-11-18-ui-style-planning/README.md).
Map directories/candidates, resource/identity scans, complete geometry and
plan/index memory remain. This is pure-style planning, not automatic structural
scheduling or native/CPU/sustained-performance acceptance. The user's fluency
priority means ordinary interactions should feel fluid and intuitive. Timing
tails remain diagnostic; no exact99%ratio or p99 gate is required.

The [typed style composition stage](RETAINED_TYPED_STYLE_COMPOSITION.md) adds
owned immutable StyleRef definitions, typed atomic rules, ordered whole-rule
overrides, Initial, pure state/environment conditions and winner diagnostics.
Shorthands normalize before duplicate checks and composition; all branches and
final combined output validate. UI input schema2 carries styles/state/environment
and separates authored from effective records. Atomic normal publication, P0/P1/
P2/layout routing, replay/recovery and old frames are preserved. Eight regular
core cases and one real-Noto case were added; retained510passed/1ignored,
text/WGPU543/40, hello55/9 and framework library Clippy pass. Explicit GTX1650Ti/
Vulkan style4 and glyph5 complete-pixel submissions match direct ordinary full
trees. See [evidence](performance/2026-11-18-style-composition/README.md).
At that composition stage full-graph resolution/copies, winner-map allocation and
candidate/global validation costs remained. The follow-up above reduces planning
and reuses accepted topology. Tokens/themes/parts/inheritance, automatic event state,
bindings, full events/focus scopes/IME/semantics, macros/ABI and automatic builder scheduling remain open;
no new timing/memory/sustained-performance qualification follows.

The [owned primitive UI input stage](RETAINED_OWNED_UI_INPUT.md) starts the
source-side UiSnapshot/UiBatch/UiDocument boundary, distinct from render packets.
Framework LayoutStyle and ID-based resources feed the ordinary retained
transaction/layout/publication path; exact baselines/sequences, bounded last-batch
replay, generation tombstones and full snapshot recovery preserve atomic state.
Messages are transferable, owner remains local; each publication owns its external
hit mapping. This is experimental Rust input, not complete semantic UI IR or a
frozen wire/foreign ABI. Logical scopes/bindings/complete event/semantics and
Store/Command/macros remain separate missing contracts. Candidate/source graph
copies, global validation and unbudgeted resource registry costs are explicit;
no throughput or sustained native performance claim follows from this API.
At that input stage: retained502passed/1ignored; text/WGPU535/39; hello54/8; explicit
GTX1650Ti/Vulkan mixed4 and actual text/resource transition8 complete-pixel
submissions match direct ordinary full trees. Framework library Clippy and
scoped framework/text format/diff pass. Ten regular core cases, one real-glyph
case and two explicit hardware cases were added. No fresh vendor/browser run
is claimed; schema/identity/replay/recovery/generation limits are covered in
[evidence](performance/2026-11-18-owned-ui-input/README.md). Software Exact
still rejects decorative shadows; hardware's extra CPU control explicitly
allows their omission, without changing clip/opacity/text/hit geometry.

The subsequent [GPU integration stage](RETAINED_GPU_INTEGRATION.md) connects
FramePacket deltas to persistent quad allocations and bounded in-flight submits.
The [clip, scroll and damage stage](RETAINED_PROPERTIES_DAMAGE.md) adds shared
property chains, normal UI scroll events and measured partial GPU redraw.
The [group-compositing stage](RETAINED_GROUP_COMPOSITING.md) adds isolated opacity,
content-cache invalidation and a correct readback bridge for normal window paint.
The [reactive update stage](RETAINED_REACTIVE_UPDATES.md) connects typed source/sink
bindings, nested event transactions, lifecycle cleanup and measured batch dispatch.
The [logical scope stage](RETAINED_LOGICAL_SCOPES.md) separates component identity
from render parentage, retaining keyed state and cleaning up portal output.
The [owned measurement stage](RETAINED_OWNED_MEASUREMENT.md) adds intrinsic image
and fixed custom inputs, baseline dependencies and actual measure-call counts.
The [responsive contract stage](RETAINED_RESPONSIVE_CONTRACT.md) adds validated
Flex styles, actual resize rules and fixed content-box container queries.
The [decorated quad stage](RETAINED_DECORATED_QUADS.md) adds uniform rounded
fills/inside borders and explicit update-route reasons.
The [material patch stage](RETAINED_MATERIAL_PATCH.md) preserves command identity
for supported solid/decorated color changes while updating the owned material.
The [visual frame queue stage](RETAINED_FRAME_QUEUE.md) coalesces pending output,
checks skipped-base resync and verifies old-snapshot completion retirement.
The [decoded image resource stage](RETAINED_IMAGE_RESOURCES.md) connects typed
resource replacement, intrinsic invalidation, shared GPU textures, ordered
image draws and completion-held texture/binding leases.
The [paragraph measurement stage](RETAINED_PARAGRAPH_MEASUREMENT.md) retains
real shaped runs, wraps against Taffy content constraints without UI callbacks,
memoizes four widths and records explicit font epochs.
The [grayscale glyph stage](RETAINED_GLYPH_RENDERING.md) attaches owned platform
masks to typed text properties, reuses packed R8 pages and persistent glyph
instance ranges, and verifies real normal-click text painting and batch uploads.
Its [material follow-up](performance/2026-11-11-glyph-materials/README.md) reduces
color-only writes to one 16-byte record per text node with zero geometry upload.
The [glyph property follow-up](performance/2026-11-11-glyph-properties/README.md)
uses 32-byte origin/scale/clip slots and 48-byte local glyph records, keeping
translation/clip changes out of the glyph geometry buffer.
The [glyph delta stage](RETAINED_GLYPH_DELTAS.md) owns immutable backend chunks,
retains unmodified draws and shared mask references, and skips per-glyph
validation/reconstruction for material/property-only updates.
The [paged Paint Store stage](RETAINED_PAINT_STORE.md) publishes local paint
changes from dirty node indices, copying only affected immutable 64-item pages
and preserving old frame snapshots.
The [subtree extraction stage](RETAINED_SUBTREE_EXTRACTION.md) extends this to
translation, clip, scroll and opacity, using indexed disjoint dirty subtrees and
the same extractor as full publication.
The [persistent property storage stage](RETAINED_PERSISTENT_PROPERTIES.md) shares
immutable AVL roots across packets, copies only changed property paths and
preserves independent transform/clip/scroll/effect identity for consumer reuse.
The [layout-output publication stage](RETAINED_LAYOUT_PUBLICATION.md) observes
actual Taffy writes, compares final results with the published layout and
patches only affected geometry/subtrees for stable visibility and order.
The [published-baseline stage](RETAINED_LAYOUT_BASELINES.md) carries the actual
first horizontal baseline through the vendored Taffy final Layout and the
framework's LayoutResult, including containers, hidden/cache transitions and
baseline-only publication without paint work; Grid final propagation is fixed.
The [intrinsic sizing/query stage](RETAINED_FLEX_SIZING_QUERIES.md) adds public
preferred-size keywords, fixes physical gap axes, resolves root sizing keywords
in the Taffy fork and separates final used-size wrapping from percentage
definiteness. Public container queries prove fixed inline extents with auto
heights and evaluate nested dependency stages. Full RFC acceptance remains open.
The [size-containment stage](RETAINED_SIZE_CONTAINMENT.md) adds framework-owned
Normal/InlineSize/Size boundaries and modifies the project-owned Taffy algorithms
to ignore contained intrinsic axes while preserving child layout. Fluid queries
support full-size external allocation and proven auto-height column chains;
staged scheduling includes sibling allocation. Positioning coordinates are
normalized from Taffy's containing block into render-parent space for full,
selective and local publication, with CPU pixel and independent browser checks.
General query graphs, complete CSS containment and full RFC acceptance remain open.
The [intrinsic-limit stage](RETAINED_INTRINSIC_LIMITS.md) changes public and
fork min/max fields to Dimension, resolves intrinsic limits inside real layout
algorithms and remeasures Flex cross limits after main allocation. It verifies
native shaped paragraphs, invalid transactions, hidden/content changes and
47 independent browser geometry assertions, with two explicit percentage-height
deviations outside the pass count. This adds capability, not a speedup claim.
The [normal-unbounded layout stage](RETAINED_UNBOUNDED_LAYOUT.md) separates
normal indefinite layout from intrinsic MaxContent in the project-owned Taffy
fork and decouples layout available space from the finite paint/hit viewport.
UiTree/ReactiveUi/LogicalUi expose resolved and local frame entry points;
viewport-only changes refresh clip output without layout. Tests cover Flex
basis/bounds/wrap, percentage/OOF bases, old frames, CPU clipping and real text;
14 independent browser block-axis geometry assertions pass. At that stage,
parent constraints/builders and structural adaptation remained open; this does not
qualify sustained presentation or close full RFC acceptance.
The [atomic responsive stage](RETAINED_ATOMIC_RESPONSIVE.md) adds public
resolved/local responsive output with independent environment/layout/paint
inputs. Viewport and nested query stages use one source draft and private
layout previews, publishing final sources/frame/effects only after validation.
Dependency proofs include all reachable computed layout sinks and exact rule
writer ownership. Errors preserve published owner state; transient restored
materials do not publish. Private tree copies and repeated final layout add
cost; graph indexing, incremental rollback, nested constraints/builders and
structural adaptation remain open. WGPU readback matches a fresh full render in an explicit
test; no new timing or sustained-presentation qualification follows.
The [parent-constraint stage](RETAINED_PARENT_CONSTRAINTS.md) adds public
BoxConstraints min/max/tight/loose intervals to LayoutConstraints and the actual
root/subtree measurement algorithms. Host bounds override authored limits and
padding floors, participate in wrapping/allocation/text measurement and keep
child bounds independent. The fork applies them to leaf/Flex/Grid/Block/OOF
paths with cache-mode separation; tests include real paragraphs and CPU pixels.
This is root/isolated-subtree host input, with nested constraint builders and
structural adaptation still open. It adds memory payload and claims no speedup.
The [nested constraint-box stage](RETAINED_NESTED_CONSTRAINT_BOX.md) adds a real
single-child parent protocol with per-axis Tight/Loose content allocation,
padding/border/gutter deflation and nested CSS Flex composition. It handles
intrinsic preferred sizes, selected-width remeasurement, baseline/overflow/OOF
publication and hidden children through the owned kernel. Normal property,
topology and unified logical edits validate arity/position before publication;
local/full CPU pixels and real paragraphs are checked. Sparse fork validation
and extra measurement add costs. General content-sized construction remains
open, with no new performance qualification.
The [constraint-builder stage](RETAINED_CONSTRAINT_BUILDERS.md) adds an explicit
parent-input builder for Size-contained constraint boxes under real Flex
allocation. Keyed branch/state/effect/binding construction and retirement join
final layout and complete frame extraction in `LogicalUi::edit_frame`; changed
observed inputs and nonfinite final geometry reject before publication. Nested
builders read the current draft, with old frame/CPU full-reference/real paragraph
regressions. Full private copies and per-builder previews add costs. General
content-sized builders, automatic dependency scheduling, combined container-query
structural plans and incremental planning remain open; no timing qualification.
The [inline builder stage](RETAINED_INLINE_CONSTRAINT_BUILDERS.md) adds a public
inline-only constraint input for InlineSize-contained boxes with content-driven
auto height. A shared proof handles Column chains, Tight parent box edges and
exact fixed-width CSS boundaries; final publication rechecks proof and values.
Nested wrapped content updates ancestors and following siblings. Real glyph
CPU pixels and explicit WGPU branch/resize readback match fresh full trees.
General full-axis builders/custom policies and incremental planning remain
open; no sustained-frame or timing qualification follows.
The [custom parent stage](RETAINED_CUSTOM_PARENT_LAYOUT.md) adds framework-owned
pure ParentLayout snapshots with direct child measurement, independent hard
constraints/percentage bases and complete placement plans. Real nested Flex and
paragraph children participate in auto height, baseline, overflow, scrolling,
local/resolved frames and atomic edit_frame failure recovery. The vendored
kernel stores delegates sparsely and reuses exact final layout caches; lossy
ComputeSize caches are bypassed throughout custom invocations because their
metadata/percentage keys are insufficient for arbitrary policies. Dirty custom
layout uses a private Taffy clone, adding O(tree) cost. Nine regular framework
cases, real Noto glyph CPU comparison and an explicitly run WGPU full-reference
check validate this subset. Full RenderBox, general structural builders,
automatic dependency scheduling, query integration and performance acceptance
remain open. Validation: retained 271 passed; text/WGPU 304 passed, 3 ignored;
hello 18 passed, 1 ignored; explicit hardware case 1 passed; Taffy default 195 /
project-config 189 passed; Clippy -D warnings passed. This stage provides
capability, without a speedup claim.
The [atomic local edit stage](RETAINED_ATOMIC_LOCAL_EDITS.md) adds
`LogicalUi::edit_local_frame` and non-resolving `published_local_frame` queries.
It reuses the keyed source/lifecycle/final-builder-validation boundary, publishing
full/inline builders and pure custom parents directly as LocalFrame snapshots.
Stable property/material edits retain sparse predecessor identity and avoid
resolved item/property-map construction; a shared finite-geometry check covers
translated own clips even when an inherited clip hides overflow. Failure leaves
owner state, effects and the old local snapshot intact. Final geometry checking
is O(N) with O(depth) temporary storage; whole-tree atomic candidates and builder
previews remain. Validation: retained 279 passed; text/WGPU 312 passed, 4 ignored;
hello 19 passed, 1 ignored; explicit hardware case 1 passed; Clippy -D warnings
passed. Real glyph CPU output and consecutive/skipped-predecessor GPU output
match independent full references. This closes atomic local edit output only;
incremental planning, general structural builders/query scheduling, complete
RenderBox and broad Flex/performance acceptance remain open. No timing or
sustained-presentation qualification follows.
The [exact custom measurement stage](RETAINED_EXACT_PARENT_MEASURE_CACHE.md)
replaces the whole-invocation ComputeSize bypass with sparse bounded complete
LayoutInput/LayoutOutput records. It preserves both percentage bases, requested
axis/definiteness/modes/bounds and baseline/overflow/margin metadata. Per-node
Arc copy-on-write shares unchanged stores across private layout previews;
exact-only measured nodes participate in ancestor invalidation, hidden/removal
and generation lifetime. Six new kernel and two framework regressions cover
correct reuse and invalidation; one versus twelve identical real Flex queries
reach equal leaf work, and repeated actual Noto paragraph measurements retain
independent full-reference glyph pixels. Validation: retained 281 passed;
text/WGPU 314 passed, 4 ignored; hello 19 passed, 1 ignored; Taffy default 201 /
project-config 195 passed; Clippy and four feature checks passed; explicit
custom-parent WGPU reference case 1 passed. Each populated store reserves nine
160-byte records in the tested default Windows ABI, plus header/Arc/map cost.
Whole-tree candidate/map cloning, global custom-invocation selection, cache
byte budgets and timing/performance acceptance remain open. This demonstrates
measurement reuse, without a frame-time/tail-latency improvement claim.
The [structural responsive stage](RETAINED_STRUCTURAL_RESPONSIVE.md) adds
`LogicalEdit::responsive` inside resolved/local atomic keyed edits. Existing
viewport and dependency-ordered container rules settle the current source and
structural draft before explicit full/inline builders. Live and newly created
derived sources/bindings/effects use final values; caught responsive failures
restore per-call source savepoints. Final publication rechecks every observed
width, selected style, exact writer and query dependency proof, including new
layout sinks added after selection. Nine regular regressions, actual Noto CPU
glyph pixels and one explicitly run WGPU full-reference test cover this subset.
Validation: retained 290 passed; text/WGPU 323 passed, 5 ignored; hello 20 passed,
1 ignored; Clippy -D warnings passed; focused structural run including hardware
10 passed. The device is GTX 1650 Ti/Vulkan, not a modern desktop-effects or
visible-presentation qualification. Existing rule registration remains outside
the edit; automatic cross-builder/query scheduling, general full-axis builders,
incremental planning/rollback and broader Flex/performance acceptance remain
open. Full previews, source-map savepoints, computed-source scans/sorts and
repeated proof/DAG validation add costs; no new timing improvement is claimed.
The [scope-owned responsive lifecycle stage](RETAINED_SCOPED_RESPONSIVE_RULES.md)
adds keyed rule declaration/update/removal and explicit registered-rule
selection inside the same atomic edit. New scopes/sources/nodes can register
viewport/container rules before full/inline construction; equal declarations
retain exact writer identity. Retirement detaches owned style writers in draft
edits and legacy reconciliation; surviving cross-scope rules cannot retain dead
sources/query hosts. Final registry validation and observed selection failures
preserve old sources, rule metadata and frames. Eight regular cases, real Noto
glyph CPU comparison and an explicit WGPU fresh-full comparison cover this
subset. Validation: retained 298 passed; text/WGPU 331 passed, 6 ignored; hello
21 passed, 1 ignored; Clippy -D warnings passed; focused scoped-rule run
including hardware 9 passed. Registry mutation clones a sparse map; declaration/final preflight adds
full previews and existing proof scans. There is no timing improvement claim.
Automatic cross-builder/query scheduling, general content-sized builders,
incremental planning/rollback and full RFC/performance acceptance remain open.
The [device-pixel output stage](RETAINED_PIXEL_SNAPPING.md) adds optional
`PixelSnap::DevicePixels` with a captured scale and atomic keyed-edit staging.
Absolute border/content/clip edges project after logical layout and glyph chunk
selection; LayoutResult, query thresholds, builder inputs and text wrapping stay
logical. CPU/WGPU/hits share projected geometry, and scale-mismatched consumers
reject before changing their state. Seven regular framework cases cover Flex,
query/builder inputs, rollback, scale, clips/scroll/effects/image/OOF and lifecycle;
actual Noto glyph CPU output and two explicit hardware cases match independent
references. Validation: retained 305 passed; text/WGPU 338 passed, 7 ignored;
hello 22 passed, 2 ignored; Clippy -D warnings passed. Every dirty snapped frame
currently forces full extraction/O(N) projection and loses ancestor placement
sharing; default None retains existing sparse routes. No speedup is claimed.
Incremental projection, host DPI integration, broad precision/baseline/rounding
conformance and full RFC/performance acceptance remain open.
The [independent parent inline stage](RETAINED_PARENT_INLINE_ALLOCATION.md) adds
an optional ParentInlineLayout phase with width-only inputs and enforced tight
child measurement/final-placement widths and separate percentage bases. Shared
inline proofs cross these edges only after proving the parent's own width;
arbitrary allocators/ancestor feedback still reject. Scope-owned and standalone
queries, inline builders, nested custom parents, real Noto text and explicit
GPU references exercise this boundary. Atomic observations retain algorithm and
ordered-child identities; changed dependencies reject despite equal widths, and
sibling order producers precede slot consumers. Full candidates, phase slot
allocation and conservative dependency snapshots/scans add costs. No speedup is
claimed. Validation: retained 314 passed; text/WGPU 347 passed, 8 ignored; hello
23 passed, 2 ignored; Clippy -D warnings passed; focused run including hardware
10 passed. General intervals/full-axis/content-sized construction, automatic
scheduling, complete RenderBox and full RFC/performance acceptance stay open.
The [incoming parent allocation stage](RETAINED_PARENT_ALLOCATION_BUILDERS.md)
adds ParentAllocationLayout with complete loose/tight intervals from incoming
border-box constraints, enforced across measurement and final placement. The
separate allocation_builder observes the host's incoming slot and permits
content-derived sizes without containment along independently allocated chains
rooted in external constraints. Full style/snapshot/ancestor-order observations
reject later feedback despite equal numbers. Sparse final records and complete
input guards preserve soft space, cached/hidden/clone/node lifetimes. Real Flex,
Noto glyph and explicit GPU controls validate output; ordinary mixed-edge proofs,
automatic scheduling, incremental previews and full RFC acceptance remain open.
Validation: retained 321 passed; text/WGPU 354 passed, 9 ignored; hello 24 passed,
2 ignored; focused allocation suite 8 passed including one explicit WGPU case;
Taffy default 203/project 197 passed; three feature checks and Clippy passed.
The [alignment stage](RETAINED_FLEX_ALIGNMENT.md) adds framework-owned
Alignment<T>/AlignmentOverflow values, logical/flex/self-relative keywords and
Safe/Unsafe positional fallback. Existing builder calls remain; direct public
field assignments require `.into()` and readers use `.position`/`.overflow`.
Default justify Start stays stable. Invalid modifiers reject before atomic
publication, while modifier-only source changes participate in layout and P0
same-value cutoff. The owned kernel fixes explicit main-axis Stretch on reversed
axes and content Stretch's overflow fallback. Independent Chromium145 geometry
has 5,364 matches within 0.02px plus twelve separately asserted spec/browser
deviations for overflowing reverse SpaceBetween; they are not compatibility
passes. Responsive resolved/local CPU, real Noto glyph/resource lifetime and
explicit WGPU pixels match independently positioned ordinary full trees. This
extends the public contract, not complete Flex conformance or timing acceptance;
see the [current validation record](performance/2026-11-17-flex-alignment/README.md).
The [last-baseline stage](RETAINED_LAST_BASELINE.md) carries independent horizontal
first/last sets through fixed/shaped leaf measurements, custom parent results,
ConstraintBox/Block/Flex propagation and LayoutResult. Public item/self alignment
adds LastBaseline; existing single-metric leaf literals remain supported. Separate
sharing groups size and anchor lines correctly, including singletons, wrap-reverse,
negative margins and logical-end overflow for cross-axis auto margins. Independent
browser marker geometry and outer-container baseline probes, normal reactive
resolved/local sources, custom-parent rollback, real Noto glyphs and explicit GPU
position controls validate the normal chain. Full baseline/writing-mode/Grid and
Flex conformance remain open. The final 1,440 browser geometry/container-baseline
cases pass without exclusions; retained 330, text/WGPU 363 (11 ignored), hello
26 (2 ignored), focused including hardware 12, vendor default/project/serde+parse
204/198/220 passed; Clippy and three feature checks passed. See the
[evidence](performance/2026-11-17-last-baseline/README.md).
The [automatic responsive session stage](RETAINED_AUTOMATIC_RESPONSIVE.md)
captures independent host environment, layout constraints and paint viewport
after atomic publication. Opted-in ordinary source/inbox transactions, legacy
append/bind/reorder/reparent/key retirement and frame APIs settle scope-owned
rules automatically; constraint/allocation builders settle before reading, with
final observation checks and no replay. Ordinary transactions publish validated
complete local output before effects. Failed environment/source/query/geometry
edits preserve the session and old frame. No incremental planner or new timing
improvement is claimed; host OS subscriptions, general cross-builder scheduling,
full RFC and sustained/presentation performance acceptance remain open. See
[validation](performance/2026-11-17-automatic-responsive/README.md).
Final checks: retained 337, text/WGPU 370 (12 ignored), hello 27 (2 ignored),
explicit environment suite 8 including GPU passed. Library Clippy passed;
additional all-targets Clippy found four existing test-only lints outside this
stage and is not counted as passed.
The [mixed Flex allocation-builder stage](RETAINED_MIXED_ALLOCATION_BUILDERS.md)
extends full-axis incoming construction across explicit Size+Layout isolation
cuts under ordinary Flex and size-contained ConstraintBox outgoing slots.
Builder input now preserves known dimensions and percentage definiteness apart
from bounds/available space/bases. Content-sized hosts remain possible inside
independent parent chains; ordinary outer reverse/wrap/RTL/aspect/decorated Flex
keeps its real layout semantics. Exact final input records, cache/hidden/clone/
node lifetimes and final structural proof checks preserve atomicity. General
ordinary feedback edges, child-style allocation, complete RenderBox and timing
acceptance remain open; this stage claims capability, not speedup. See
[validation](performance/2026-11-17-mixed-allocation-builders/README.md).
Final checks: retained 343, text/WGPU 376 (13 ignored), hello 28 (2 ignored),
explicit mixed suite 7 including GPU passed. Taffy default/project suites passed
206/200; no-default, alloc/Flex and all-feature checks, library Clippy and diff
checks passed. Offscreen GTX1650Ti output is not sustained-presentation evidence.
The [parent-data stage](RETAINED_PARENT_DATA.md) adds typed immutable child
metadata, read-only allocation/measurement views and ordinary source/property
updates. Independent full allocation may use direct-child data without child
style/measurement; count-only implementations remain supported. Final builder
observations retain all allocator child-data identities, and computed metadata
sinks participate in both responsive planners. Hidden/reparent/clone/retirement,
coalescing, invalid rollback, real Noto wrapping, ordinary full Flex CPU controls
and explicit local WGPU readback cover this input domain. Sparse kernel maps,
framework Node/Pending handles and conservative invalidation/proof costs grow;
no timing improvement, complete RenderBox or general feedback proof is claimed.
See [validation](performance/2026-11-17-parent-data/README.md).
Final checks: retained 351, text/WGPU 384 (14 ignored), hello 29 (2 ignored),
explicit parent-data suite 9 including actual GPU passed. Taffy default/project
suites 207/201, three feature checks, library Clippy and diff checks passed.
No all-targets Clippy or additional browser conformance pass is claimed.
The [dry parent measure stage](RETAINED_DRY_PARENT_MEASURE.md) adds an optional
owned ParentMeasure phase with size/first/last content metrics and exact final
plan agreement. Actual size probes skip custom placement-plan construction;
existing complete-plan parents retain fallback. Explicit dry_measure_node on
UiTree/ReactiveUi/LogicalUi takes child constraints plus separate percentage
bases, runs ComputeSize on a private candidate and leaves published state/cache
untouched. The older full-root measurement query stays unchanged. Nested hosts,
inline/full enforcement and metadata, intrinsic/bounds/contain/aspect controls,
error rollback, true Noto pixels and actual WGPU readback cover the subset.
Final metric queries add work, public clones and normal baseline scratch writes
remain; callback reductions do not establish timing or complete RenderBox.
See [validation](performance/2026-11-17-dry-parent-measure/README.md).
In the four-width client control, complete plans fall from26 to4, with26 pure
metric queries; this is not a wall-time claim. Final regression: retained359;
text/WGPU392 passed/15 ignored; explicit dry9 including actual GPU; hello30
passed/2 ignored; Taffy209/203. Library Clippy and kernel feature checks pass.
The [explicit baseline query](RETAINED_DRY_BASELINE_QUERY.md) separates complete
supported-kernel first/last metrics from size-only measurement. Public UI queries
and custom parent callbacks reuse final placement arithmetic under scoped
geometry/cache/observation/OOF write suppression. No baseline-specific layout
algorithm, persistent cache, NodeData/GPU/packet fields or performance claim is
introduced; queries may do substantial repeated work. See
[validation](performance/2026-11-17-dry-baselines/README.md). Full RenderBox,
baseline/intrinsic conformance and remaining RFC gates stay open.
Final regression: retained364; text/WGPU397 passed/16 ignored; explicit baseline6
including real GPU; hello31 passed/2 ignored; Taffy212/206. Library Clippy and
kernel feature checks pass. An initial concurrent LLVM out-of-memory compile
was rerun sequentially; it is not counted as a passed test invocation.
The [owned container overflow fix](RETAINED_OWNED_CONTAINER_OVERFLOW.md) replaces
the host sizing helper's phantom content extent with actual child contributions
and scroll end padding once, for both custom parents and ConstraintBox. The prior
baseline fixture now checks full LayoutResult equality. ParentMeasure/ParentLayout
baselines are finite signed content offsets, including negative/beyond-height
positions; fixed leaf intrinsic validation remains separate. Finite384 kernel
controls, reachable OOF descendants, empty/hidden lifecycle, resolved/local scroll
reclamping, cached descendants, hit/pixel and real text controls are recorded in
[validation](performance/2026-11-17-owned-container-overflow/README.md).
Final regression: retained366; text/WGPU399 passed/17 ignored; hello32 passed/2
ignored; explicit owned-overflow3 including actual GPU passed; Taffy216/210.
Library Clippy, kernel feature and diff checks pass. This is a correctness fix
with no new frame-time or sustained-presentation qualification.
The [content measurement stage](RETAINED_CONTENT_MEASURE.md) exposes owned pure
constraint-dependent content leaves through the normal measure property. Soft
content space retains normal/intrinsic modes; the common leaf model selects
inline size before dependent height, publishing natural overflow and finite
signed baseline metrics. A sparse authored kernel capability enables complete
cache keys throughout invocations, and fallible layout stages a private tree and
observer journal. Typed sources/no-op, nested custom parents, intrinsic/limits/
containment/direction, atomic failure, old owners and real Noto metrics cover
this protocol. See [validation](performance/2026-11-17-content-measure/README.md).
Final regression: retained376; text/WGPU409 passed/18 ignored; hello33 passed/2
ignored; explicit content11 including actual GPU passed; Taffy217/211.
Library Clippy, kernel feature and diff checks pass.
It adds capability and clone/cache/measurement costs; full RenderBox, general
feedback construction and performance acceptance remain open.
The [measured child-style stage](RETAINED_CHILD_STYLE.md) exposes owned unresolved
item inputs to pure metrics/full parent policies, including effective replaced
classification. Real kernel styles and existing ancestor invalidation supply
this domain with no persistent mirror. Independent allocators and structural
proof domains remain restricted. Finite client wrapping controls compare actual
nested Flex geometry/CPU pixels, preserving the protocol's zero-margin metadata
difference and explicit f32 tolerance; real Noto glyphs compare full results.
See [validation](performance/2026-11-17-child-style/README.md). Independent style
allocation/proofs, complete RenderBox and full RFC/performance acceptance remain
open, with no speedup claim.
Final regression: retained382; text/WGPU415 passed/19 ignored; hello34 passed/2
ignored; explicit child-style7 including actual GPU passed; Taffy218/212.
Library Clippy, kernel feature and diff checks pass.
The [independent allocation style stage](RETAINED_ALLOCATION_STYLES.md) extends
the full read-only allocator with item styles, while retaining no measurement
capability. Structural observations now capture every direct child's effective
style at every allocator ancestor, including image-derived classification/aspect
and siblings, even for unchanged numerical slots. Existing query stages include
computed style sinks affecting sibling allocation. Finite normal/local keyed
builders, failure recovery, nested dependencies, hidden/order/reparent and actual
Noto glyphs cover the contract. General feedback construction/query proofs,
complete RenderBox, incremental planning and performance acceptance remain open.
See [validation](performance/2026-11-17-allocation-styles/README.md).
Final regression: retained388; text/WGPU421 passed/20 ignored; hello35 passed/2
ignored; explicit allocation-style7 including actual GPU passed; Taffy218/212.
Library Clippy, kernel feature and diff checks pass. Project-feature kernel tests
required a lower-memory test-profile retry after an LLVM compile out-of-memory;
the reproduction record preserves both the failure and successful settings.
The [selected child assignment stage](RETAINED_CHILD_ASSIGNMENTS.md) adds
known_size and known_size_is_definite to ChildLayoutInput, preserving real
selected dimensions independently from hard intervals/soft space/incoming
percentage bases. Readback no longer discards known dimensions; enforced
assignments, measured plans, nested allocation inputs, keyed builders and
explicit dry queries share the domain. Finite source-driven definite/indefinite
percentage children, ordinary full CPU controls, restored P0 writes, failure
rollback and actual Noto wrapping cover normal routes. Experimental struct
literals are migrated; public temporary input payload grows, with no kernel
cache, persistent node, GPU or packet format change. No speedup is claimed.
See [validation](performance/2026-11-17-child-assignments/README.md). Complete
RenderBox/general feedback/scheduling and full performance acceptance remain open.
Final regression: retained394; text/WGPU427 passed/21 ignored; hello37 passed/2
ignored; explicit selected-size7 including actual NVIDIA/Vulkan passed. Framework
library Clippy, new test formatting and diff checks pass. No kernel source or
feature configuration changed, so no new standalone vendor suite is claimed.

The [explicit content-sizing stage](RETAINED_CONTENT_SIZING.md) adds
LayoutSizingMode::Inherent/Content to child inputs and parent/full/inline/builder
callback inputs. Content contribution queries ignore the queried node's own
preferred/min/max without erasing descendant rules; selected/hard inputs and
insets remain. The shared kernel child entry no longer resolves own sizing
keywords in ContentSize, and owned boxes suppress their own aspect suggestions.
Existing ordinary/exact cache modes remain authoritative. Finite query matrices,
normal source updates/keyed builders, CPU full controls and true Noto glyphs
exercise both modes. Temporary input payload grows with no persistent cache,
node/GPU/packet format growth or speedup claim. Complete RenderBox, general
contribution/cyclic sizing, scheduling and performance acceptance remain open.
See [validation](performance/2026-11-17-content-sizing/README.md).
Verified: retained400; text/WGPU433 passed,22 ignored; explicit content suite7
including actual NVIDIA/Vulkan; hello39 passed,2 ignored; vendor default219 /
project213. Framework library Clippy, vendor no-default/alloc-Flex/all-features,
new framework/glyph test formatting and diff checks pass. These suites overlap;
ordinary ignored hardware cases are not counted as passing.

The [Flex automatic minimum stage](RETAINED_FLEX_AUTOMATIC_MINIMUM.md) preserves
authored preferred-size provenance instead of eagerly filling Auto axes through
aspect ratio. Replaced/ordinary content use the distinct content/transferred
suggestions; explicit cross sizes retain their own limits. Ratio transfers use
the actual BorderBox/ContentBox, and final automatic cross sizing retains normal
content minima and overflow behavior. The existing image classification and
normal Flex/source/layout/CPU/GPU routes are reused, without new public style or
persistent node/GPU/packet fields. Transient FlexItem inputs and arithmetic grow;
no performance improvement is claimed. Independent browser geometry, normal typed
updates, decoded image resources and true glyph controls validate finite cases.
See [verification](performance/2026-11-17-flex-auto-min/README.md). Complete
RenderBox, broad aspect/Flex/CSS and sustained visible-frame acceptance stay open.
Verified: retained403; text/WGPU436 passed,23 ignored; explicit suite4 including
NVIDIA/Vulkan; hello40 passed,2 ignored; vendor default220 / project214; framework
library Clippy and vendor no-default/alloc-Flex/all-features checks pass. The816
browser cases include804 Chromium145 matches and12 separately asserted normative
cross-limit deviations. New test formatting and diff checks pass; suites overlap.

The [intrinsic containment fallback stage](RETAINED_CONTAINMENT_FALLBACK.md)
adds optional finite nonnegative inner lengths to public LayoutStyle and
read-only ChildLayoutStyle. The common leaf/contained box model preserves them
across ordinary Flex/Grid/Block, owned custom parents, ConstraintBox and Content
queries. Normal source updates, independent allocator dependency guards,
responsive thresholds, text/images and complete CPU/GPU output retain existing
routes. No fictitious child/overflow or new preferred dimension is introduced.
Each stored style costs16 additional bytes on x64; remembered-size/skipping,
automatic scrollbar prediction and broad conformance remain open. See
[verification](performance/2026-11-17-containment-fallback/README.md).
Verified: retained412; text/WGPU445 passed,24 ignored; explicit NVIDIA/Vulkan
case1; hello41 passed,2 ignored; vendor default221 / project215; framework
library Clippy and no-default/alloc-Flex/all-features checks pass. The840
Chromium145 Flex geometry cases all match. Suites overlap; finite offscreen
correctness is not sustained or visible-frame performance qualification.

The [Flex percentage-gap stage](RETAINED_FLEX_PERCENTAGE_GAPS.md) uses existing
selected-size definiteness when resolving physical-axis gap percentages. A
content-derived block extent no longer creates cyclic gaps. Normal fit-content
inline layout rechecks line membership once after a selected width resolves a
changed percentage gap, preserving that width and the ordinary greedy/balanced
collectors. Normal source updates, measured/independent parent assignments, dry
baselines/hard bounds, real glyphs and full CPU/GPU numeric-gap controls share
the kernel route. No public/persistent/GPU/packet field is added; scalar checks
and a conditional extra item scan/line allocation cost remain unprofiled.
See [verification](performance/2026-11-17-flex-percentage-gaps/README.md).
The1080 independent cases include1008 Chromium145 geometry matches and72
separately asserted normative wrapped-Column MinContent deviations. Complete
cyclic/CSS/RenderBox, scheduling and sustained visible-frame acceptance stay open.
Verified: retained416; text/WGPU449 passed,25 ignored; explicitly run
NVIDIA/Vulkan case1 with15 full-pixel submits; hello42 passed,2 ignored;
vendor default223 / project217; framework library Clippy and vendor
no-default/alloc-Flex/all-features checks pass. Scoped formatting and diff
checks pass; suites overlap and ignored tests are not counted as passed.

The [owned parent provenance stage](RETAINED_PARENT_DEFINITENESS.md) adds
known_size_is_definite to ParentMeasure/ParentLayout inputs and inline width
provenance to ParentInlineInput. The common custom-parent and ConstraintBox
final paths preserve explicitly indefinite incoming widths instead of treating
all used widths as percentage bases; published child padding/border uses the
same actual basis. Normal auto inline selection and hard-tight assignments keep
their existing authority, while a known final content-derived height remains
indefinite. Normal typed sources, dry/cache queries, pure policy rejection with
atomic rollback, real paragraphs and complete CPU/GPU numeric controls validate
the finite protocol. Temporary callback structs gain bools; persistent node,
style/cache/GPU/packet formats and callback-pass count do not grow. Full RenderBox,
general content feedback, scheduling and performance acceptance remain open.
See [verification](performance/2026-11-17-parent-definiteness/README.md).
Verified: retained420; text/WGPU453 passed,26 ignored; hello43 passed,2 ignored;
explicit NVIDIA/Vulkan case1 with10 complete-pixel submit comparisons;
vendor default224 / project218; framework library Clippy and vendor
no-default/alloc-Flex/all-features checks pass. Scoped rustfmt and diff checks
pass. Suites overlap; ignored hardware cases are not passing evidence.

The [axis measurement stage](RETAINED_AXIS_MEASUREMENT.md) exposes LayoutAxis,
scalar dry queries on public owners, child measure_axis and requested_axis in
ParentMeasure inputs. Width-only custom probes omit selected-width dependent
height and final metric/plan work; vertical probes retain necessary inline
selection. Full/inline assignments preserve allocation authority independently
from requested demand. Complete plans, final child placements and baseline
queries normalize to Both, with exact cache separation from partial metrics.
Normal sources, P0 restoration, actual glyphs and complete CPU/WGPU numeric
controls share the ordinary route. Callback inputs grow, while ChildLayoutInput,
builder/style/persistent cache/GPU/packet formats do not. Public queries still
clone the tree and no timing gain is claimed; full RenderBox, general intrinsic
conformance, feedback construction/scheduling and performance acceptance stay open.
See [verification](performance/2026-11-17-axis-measurement/README.md).
Verified: retained426; text/WGPU459 passed,27 ignored; hello44 passed,2 ignored;
explicit NVIDIA/Vulkan case1 with6 complete-pixel submits; vendor default225 /
project219; framework library Clippy and vendor no-default/alloc-Flex/all-features
checks pass. Scoped formatting and diff checks pass. The ordinary Flex matrix
has96 input combinations/192 scalar comparisons, not independent browser cases.
Suite counts overlap; ignored tests alone are not hardware evidence.

The [content axis stage](RETAINED_CONTENT_AXIS_MEASUREMENT.md) adds requested
demand to ContentMeasureInput and a shared axis-aware leaf callback entry.
Internal intrinsic width/height limit probes keep their own demand, independent
of the outer scalar request. Width-only content probes omit an unrelated
selected-width height pass; vertical probes retain required inline selection.
Final layout/baseline queries obtain complete natural size, overflow and metrics.
The legacy leaf callback wraps the same algorithm. Finite intrinsic/ratio/
containment/gutter controls, normal source/P0/rollback/old-frame tests, actual
paragraph metrics and complete CPU/GPU ordinary-Flex controls exercise the
protocol. Temporary content inputs grow without new persistent node/style/cache/
GPU/frame fields. Public private-tree clones and other costs remain, with no
timing or full RenderBox acceptance claim. See
[verification](performance/2026-11-17-content-axis/README.md).
Verified: retained430; text/WGPU463 passed,28 ignored; hello44 passed,2 ignored;
explicit NVIDIA/Vulkan case1 with4 complete-pixel submissions; vendor default227 /
project221; framework library Clippy and vendor no-default/alloc-Flex/all-features
checks pass. Scoped framework/glyph/vendor-test formatting and diff checks pass.
Suite counts overlap; finite offscreen correctness does not qualify visible FPS.

The [ordinary inline allocation builder stage](RETAINED_INLINE_ALLOCATION_BUILDERS.md)
adds incoming inline-only construction for normal uncontained Flex/ConstraintBox
hosts. A complete Column/Tight-box/independent-width chain and an actual external
inline anchor keep constructed content height out of incoming inline inputs.
The shared input now includes selected width/definiteness and sizing mode;
final proof/input/algorithm/order checks join source/keyed/frame atomicity.
Opt-in Taffy final-input records use existing exact caches and retire at the
transaction boundary. Ordinary unchanged frames remain0 layout, while repeated
builder transactions still pay forced observation/previews. Full/local CPU,
source/binding/effect rollback, real Noto glyphs and explicit GPU references
exercise this subset. Extra sparse maps, exact cache use, ancestor scans and
private candidates remain; no timing or memory gain is claimed. See
[verification](performance/2026-11-17-inline-allocation-builders/README.md).
General Row/wrap/aspect and full-axis feedback proofs, automatic cross-builder
scheduling, complete RenderBox and sustained visible-frame acceptance stay open.
Verified: retained436; text/WGPU469 passed,29 ignored; hello45 passed,2 ignored;
explicit NVIDIA/Vulkan case1 with3 complete-pixel submissions; vendor default228 /
project222; framework library Clippy and vendor no-default/alloc-Flex/all-features
checks pass. Scoped formatting and diff checks pass. Suites overlap; ignored
hardware alone is not passing evidence.

The [explicit scrollbar allocation stage](RETAINED_SCROLLBAR_GUTTERS.md) adds
public scrollbar_width and actual LayoutResult.scrollbar_size, and corrects
content_size to exclude reserved gutters. Normal Flex/ConstraintBox/custom
parent slots, content queries and builders share the existing kernel box model.
Paragraph baseline queries, glyph width and image/glyph destinations now use
that same deflated content box, including RTL left gutters. Gutter/direction
changes refresh own content geometry even when the outer box is unchanged;
typed source validation/P0/rollback, scroll reclamping and old frames keep their
normal routes. All288 independent Chromium145 Flex cases match; actual Noto and
numeric-padding CPU/GPU controls exercise output. Public payload/dirty checks
grow while internal Style/Layout/GPU/packet/cache formats do not. No timing or
memory improvement is claimed. Automatic bar appearance/native chrome, general
feedback/scheduling/RenderBox and full performance acceptance remain open. See
[verification](performance/2026-11-17-scrollbar-gutters/README.md).
Verified: retained442; text/WGPU475 passed,30 ignored; hello46 passed,3 ignored;
explicit image/glyph NVIDIA/Vulkan cases1+1 with5+10 complete-pixel submissions;
vendor default228/project222; framework library Clippy and vendor no-default/
alloc-Flex/all-features checks pass. Scoped formatting/diff checks pass. Counts
overlap; finite offscreen pixels are not sustained visible-frame qualification.

The [ordinary container-query stage](RETAINED_ORDINARY_CONTAINER_QUERIES.md)
extends existing ContainerInline to uncontained hosts through complete independent
Column/Tight-box/width-phase chains with a live external inline anchor. Actual
content width remains distinct from incoming slots and environment/paint sizes;
descendant height changes propagate normally. Nested producer DAG ordering,
computed self-feedback rejection, scope-owned sessions and builder composition
retain atomic source/tree/frame/effect publication. Standalone responsive drafts
now recheck observed widths and custom algorithm/order dependencies against final
output, matching keyed final validation. Real Noto and ordinary full CPU/GPU
controls exercise the subset; no new public field or kernel/GPU/cache format is
added. Proof/style copies, observation vectors and private candidates add costs,
without timing/memory claims. General Row/wrap/aspect/full-axis feedback, automatic
cross-builder scheduling, RenderBox and full performance acceptance remain open.
See [verification](performance/2026-11-17-ordinary-container-queries/README.md).
Verified: retained449; text/WGPU482 passed,31 ignored; hello47 passed,3 ignored;
explicit NVIDIA/Vulkan case1 with4 complete-pixel submissions; framework library
Clippy and scoped rustfmt/diff checks pass. Vendor code/features are unchanged,
so no new vendor run is claimed. Counts overlap; finite offscreen pixels do not
qualify sustained or display-visible performance.

The [content-independent Row allocation stage](RETAINED_ROW_INLINE_ALLOCATION.md)
extends shared inline builder/query proofs across Row/RowReverse and all wrap
modes when every direct in-flow item has numeric basis (or auto+numeric width),
explicit numeric main minimum, numeric/auto maximum and no effective ratio.
Parent inline allocation must itself be independent. Normal Flex still owns
line/grow/shrink/cross geometry; no custom split-pane solver is introduced.
Final observations capture ordered siblings and effective item styles, including
image-derived ratios; responsive DAGs include sibling style producers and reject
computed allocation feedback. Real Noto/full CPU and explicit GPU references
exercise atomic publication;288 Chromium145 cases cover padding/margins/min/max,
reversal/wrap/RTL/box sizing and height changes. Public APIs/kernel/persistent
formats do not grow, but ancestor sibling scans and Style copies add costs.
General content/percentage/ratio/intrinsic allocation proofs, full-axis feedback,
automatic scheduling/RenderBox and full performance acceptance remain open. See
[verification](performance/2026-11-17-row-inline-allocation/README.md).
Verified: retained456; text/WGPU489 passed,32 ignored; hello48 passed,3 ignored;
explicit NVIDIA/Vulkan case1 with4 complete-pixel submissions; framework library
Clippy and scoped formatting/diff checks pass. All288 browser cases pass without
exclusions. No vendor code/features changed or new vendor run claimed. Counts
overlap and finite offscreen correctness does not qualify sustained presentation.

The [definite percentage Row stage](RETAINED_PERCENTAGE_INLINE_ALLOCATION.md)
extends complete incoming builder/query proofs to Percent basis, Auto+Percent
preferred width, explicit Percent minima and Auto/Px/Percent maxima. A separate
ordinary-Flex definiteness proof composes Row post-flex sizes and numeric or
stretched Column widths, conditional on a live external root anchor. Existing
unconditional used-width proofs keep their previous contract; custom/box
protocols conservatively stop the new percentage proof. Normal Flex allocation,
sibling producer DAGs, observations and atomic publication remain shared.432
Chromium145 cases all match within0.06px, with percent insets/limits, wrapping,
reverse/RTL and two box models/heights. Nested builders, automatic source query
resettlement, unanchored rejection, numeric/tight anchors, computed feedback and
late equal-width dependency rollback have regressions; real glyph/full CPU and
explicit GPU controls are in [evidence](performance/2026-11-17-percentage-inline-allocation/README.md).
No new public/persistent/kernel field or layout algorithm, and no timing gain.
Percentage proof is lazy for numeric-only inputs; extra ancestor/style scans,
private candidates/previews and final checks retain costs. General content,
intrinsic/ratio/protocol/cyclic proofs, full-axis feedback, RenderBox, automatic
cross-builder scheduling and native/CPU/sustained performance acceptance remain open.
Verified: retained466; text/WGPU499 passed,33 ignored; hello49 passed,3 ignored;
explicit NVIDIA/Vulkan case1 with3 complete-pixel submissions;432 browser cases,
framework library Clippy and scoped formatting/diff checks pass. No vendor code
or features changed; no new vendor suite result claimed. Counts overlap.

The [public Grid stage](RETAINED_PUBLIC_GRID.md) exposes framework-owned track
min/max, Fr/percent/intrinsic/fit-content sizes, Count/AutoFit/AutoFill templates,
implicit patterns, dense placement, signed lines/spans and inline item alignment.
Grid/Flex nesting, typed track/placement/order/display edits and keyed topology
use existing persistent layout/atomic frame publication. Optional Arc track
snapshots avoid Grid vectors in Flex defaults, while lowering/candidates retain
scan/allocation costs. Grid hosts can use proved external inline builders and
fixed/contained queries; general content-dependent Grid parent edges still reject.
All1,152 Chromium145 reference cases match within0.06px without exclusions after
fixing a real kernel minimum outer-contribution floor below padding+border.
Real Noto wraps/baselines and CPU glyph pixels match independently positioned
controls; explicit GPU and complete checks are in
[evidence](performance/2026-11-17-public-grid/README.md). No timing claim. Named
tracks/areas, subgrid, Grid-specific dependency proofs, broad CSS/baselines,
calc, full RenderBox and sustained/native/CPU performance acceptance remain open.
Verified: retained473; text/WGPU506 passed,34 ignored; hello50 passed,4 ignored;
explicit NVIDIA/Vulkan Grid4 and real glyph8 complete-pixel submissions; vendor
default229/project223 library tests and no-default/alloc+tree+Flex/all-features
checks; framework library Clippy and scoped formatting/diff pass. Counts overlap.

The [owned linear calc stage](RETAINED_OWNED_CALC.md) adds finite px+percentage
coefficients to public Length/Dimension, mixed fit-content and Grid track limits,
preserving basis provenance, signed margin/inset and nonnegative used domains.
Optional vendored affine tags own values directly without external resolver or
registry lifetime; custom child styles and normal Flex/Grid/ConstraintBox/text
reuse shared resolution. Definite-chain builders/queries support mixed Row inputs
under live root anchors.336 Chromium cases match within0.1px (actual max0.08px,
recorded rounding/fixture history); normal typed coefficient/P0/rollback and
numeric CPU/glyph/GPU controls are in
[evidence](performance/2026-11-17-owned-calc/README.md). Memory cost is explicit:
64-bit length8→16 and Style<String>576→776bytes, including feature-unified GPUI;
standalone disabled feature keeps original size. No wall-time or peak-memory
speedup claim. Complex expression functions/units, broad cyclic/intrinsic sizing,
full RenderBox/scheduling, incremental candidates and native/CPU/sustained
performance acceptance remain open.
Verified: retained478; text/WGPU511 passed,35 ignored; hello51 passed,5 ignored;
explicit NVIDIA/Vulkan mixed4 and glyph8 complete-pixel submissions; vendor
default229/affine+serde234/project-affine225 tests, no-default/alloc+tree+Flex+affine/
all-features checks, framework library Clippy and scoped format/diff pass.

The [Grid inline allocation stage](RETAINED_GRID_INLINE_ALLOCATION.md) extends
existing builders/ContainerInline to ordinary Grid items under content-independent
definite/fr columns and complete live-anchor parent proofs. Explicit implicit
column patterns, fixed row counts, all reachable sibling styles and numeric or
normal stretch item sizes preserve independent width while content changes rows.
Shared definiteness now composes nested Grid and Row percentage-basis descendants.
Full Grid parent Style/order/effective sibling observations and sibling-producer
query DAG edges preserve final validation and reject cycles/equal-width input
replacement.1,152additional Chromium cases all match within0.06px without exclusions;
nested builders, automatic source queries, rollback, real Noto/full CPU and GPU
controls are in [evidence](performance/2026-11-17-grid-inline-allocation/README.md).
No public/kernel/persistent format or layout pass added. Ancestor/style scans and
full track-vector observations add cost; no timing/memory gain claimed. General
intrinsic/automatic columns, ratio feedback, auto-repeated rows, named placement,
full-axis construction, RenderBox/scheduling and full performance acceptance remain open.
Verified: retained486; text/WGPU519 passed,36 ignored; hello52 passed,6 ignored;
explicit NVIDIA/Vulkan Grid item4 and real glyph8 complete-pixel submissions;
framework library Clippy, scoped format/diff and all1,152browser cases pass.
Counts overlap; no vendor code/features changed or new vendor suite claimed.

The [immutable layout-style stage](RETAINED_SHARED_LAYOUT_STYLES.md) shares
lowered authored snapshots between framework/Taffy nodes, private candidates,
dry queries and builder dependency observations. Style writes replace immutable
handles; image-derived replacement/ratio metadata remains a separate effective
snapshot only when required. Value equality/P0, cache/capability invalidation,
ordinary Flex/Grid algorithms and atomic rollback stay authoritative. Heap
allocation, pointer indirection, atomic handle costs and old-snapshot lifetimes
are explicit. Whole-tree slots/topology/caches, changed-style lowering and proof
scans remain. See [comparative CPU evidence](performance/2026-11-17-shared-layout-styles/README.md);
neither static Node size nor finite timings establish peak/RSS/native frame-rate
acceptance. The full goal remains active.
Verified: retained489 passed/1ignored; text/WGPU522 passed/37ignored; hello52
passed/6ignored; fork default232/affine+serde237/project-affine228 tests,
alloc+tree+Flex+affine/all-features checks and framework library Clippy pass.
Explicit NVIDIA/Vulkan Grid item4 and real glyph8 complete-pixel submissions
match complete references. Three regular framework and three fork regressions
plus one manual release benchmark were added. Suites overlap; the ignored
benchmark is separate from hardware skips. No new browser/conformance run is
claimed for this storage-only change. Retained-only unused fields/dependency
linker messages remain. Static styles remain776bytes, ordinary core Node
1168→400 and Slot1176→408bytes, excluding new shared heap payload/control blocks.
Four alternating200-sample release process pairs on Ryzen4800H measure median
per-process p50 reductions: Flex/Grid candidate clone55.5%/64.5%, isolated
subtree54.8%/51.2%, atomic edit+layout validation31.9%/29.8%, isolated root
measure15.7%/17.5% and create+measure28.1%/26.5%. Raw per-process p50/p95/max,
source harness and binary hashes are saved; no individual-sample/p99, peak/RSS,
live-layout-only or native presentation claim is made.

The [Grid item style-view stage](RETAINED_GRID_ITEM_STYLE_VIEW.md) extends the
same framework-owned ChildLayoutStyle with numeric column/row placements and
justify-self/safety in measured and independent parent phases. The kernel view
preserves authored names and becomes Clone when Grid is enabled; the public
numeric model explicitly rejects names/line0/span0 rather than losing input.
Full effective-style observations already cover equal-size late changes.
Test-only explicit-cell clients match normal Grid geometry, CPU pixels and
actual Noto wrapping/baselines; no new runtime solver or feedback proof is added.
Verified: retained492passed/1ignored, text/WGPU525/38, hello53/7; vendor
default233/affine+serde238/project-affine229passed; alloc+tree+Grid/Flex/Block+
affine and all-features checks, framework library Clippy and scoped format/diff
pass. Explicit GTX1650Ti/Vulkan item5 and actual
glyph8 complete-pixel submissions match normal full Grid. Three regular core,
one vendor and one real-text case plus two hardware cases were added. No timing
or memory/FPS gain is claimed. Public named styles, general Grid/custom/full-axis
feedback, RenderBox, automatic scheduling, incremental planning and full native
performance acceptance remain open.

The [validation-cache stage](RETAINED_VALIDATION_CACHE.md) reuses consumer checks
for cache-owned immutable pages and stable node indices, with full fallback for
identity/order/scale changes and complete effect-reference checks on effect-map edits.
The [hit-index stage](RETAINED_HIT_INDEX.md) routes ordinary hit testing through
persistent ordered page bounds, with lazy construction, selective page/path
maintenance and explicit linear-scan/cold/overlap performance controls.
The [image-delta stage](RETAINED_IMAGE_DELTAS.md) retains unmentioned image draws,
updates shared texture ownership by net reference changes and handles logical
resync/resize without needless texture upload.
The [composition-cache stage](RETAINED_COMPOSITION_CACHE.md) retains root draw
order and shared layer draw arrays for stable mixed primitive/effect topology,
with current opacity values and unchanged content-invalidation checks.
The [computed DAG stage](RETAINED_COMPUTED_DAG.md) propagates explicit derived
dependencies in rank order, cuts off equal outputs, settles transaction reads
and atomically publishes derived sources with typed sinks and keyed lifetimes.
The [post-commit effects stage](RETAINED_EFFECTS.md) observes final dependencies
once per successful transaction and owns replacement/scope/drop cleanup.
The [source-delivery stage](RETAINED_SOURCE_DELIVERY.md) coalesces background
Send values before owner transactions, integrates executor readiness/waking and
checks bounded queues, stale sources, actual worker threads and native execution.
The [idle GPU completion stage](RETAINED_GPU_COMPLETIONS.md) wakes the host from
native completion waits, retires snapshots/resources without another frame and
handles subscription closure and device-epoch replacement.
The [existing-tree structural stage](RETAINED_STRUCTURE_TRANSACTIONS.md) validates
final topology atomically, batches multi-host keyed reorders into one revision,
and moves portals while retaining scope/source/render identity.
Its removal follow-up preflights whole current-subtree deletion, final host
order and mixed key retirement before cleanup, publishing one render revision.
The [unified edit stage](RETAINED_EDIT_TRANSACTIONS.md) stages render construction,
final topology/removal, properties, existing source/computed updates and new
bindings behind one validation boundary and revision, with independent logical
ownership, failure rollback, stale provisional handles and real gallery events.
Its endpoint follow-up stages unbinding/rebinding in the same draft, skips
detached sinks, cancels provisional bindings and preserves old subscriptions on
failure; equal-value replacement changes no render revision or frame intent.
The [lifecycle extension](RETAINED_LIFECYCLE_EDITS.md) constructs/retires keyed
scopes, writable/computed sources, effects, bindings and output atomically,
preflights surviving dependencies/portals, cancels draft lifetimes and defers
callbacks until all logical/reactive/render ownership maps publish.
Its measured scan follow-up skips unnecessary property replay during
construction, indexes draft binding endpoints and ranges named state/effect
retirement by scope; full topology and extraction work remain in acceptance.

The [rounded-box shadow stage](RETAINED_SHADOWS.md) adds immutable drop/inset
shadow properties, ordinary typed gallery updates, clipped overflow damage and
persistent 112-byte shadow ranges. Quad and Shadow share typed GPU storage,
geometric growth and completion-held replacement leases; cropped target shaders,
conservative Scene culling and empty fractional clips have hardware checks.

The [shared GPU placement stage](RETAINED_GPU_SPATIAL.md) keeps Quad/Shadow
geometry local and shares hierarchical layout/translation/scroll/rectangular
clip slots, with bounded deep anchors and completion-held property buffers.
It validates local/resolved snapshots, depth-changing reparenting and reuse,
fixes pixel-center clipping across target sizes, and measures the complete
owner commit/publication/submission/completion path against world instances.

The [page ownership stage](RETAINED_PAGE_OWNERSHIP.md) moves complete changed
paint pages instead of cloning their old payloads, keeps dense candidates
contiguous and shares authoritative immutable pages with sorted delta views.
Exact root identity skips duplicate delta comparisons, while public COW mutation
uses checked item lists. Original selective copying remains an A/B control;
phase measurements distinguish commit, publication and submission costs.

The [submitted-snapshot routing stage](RETAINED_CONSUMER_ROUTING.md) reuses
unchanged local Quad/Shadow inputs, updates only changed spatial records and
bounded anchors, and retains original preparation as an A/B control. Private
normal-producer provenance ties complete deltas to the actual submitted base;
unknown or modified transitions force full snapshot recovery, including missing
removals/damage. Conflicting same-revision roots are rejected before equal-frame
reuse. Hardware and phase measurements compare both consumer modes.

The [backend spatial index stage](RETAINED_SPATIAL_INDEX.md) retains validated
shader-parent adjacency/resolved values and inverse Quad/Shadow property users.
Stable updates check affected descendants and projected references, while
capacity/scale changes and invalidated aggregate bounds rebuild from actual
mirrors. CPU plans commit only after all preflight checks pass. Indexed raster
dependencies preserve the original full-scan A/B control, including adversarial
raw batches, unreferenced overflow, prefix reexposure and rejection/retry checks.
The measured unconditional prototype is retained; dense frontiers use full-prefix
validation while sparse frames retain indexed reference checks. The later dense
backend stage replaces repeated full ancestry walks with depth-ordered resolution;
the original walk remains an explicit control.
Resolved memos are invalidated across dense changes, with bounded actual-mirror
ancestor reconstruction on following sparse frames.

The [shared content placement stage](RETAINED_SHARED_CONTENT.md) adopts the same
GPU hierarchy for images and glyphs, preserves ordered immutable content insets,
keeps the existing 48/32-byte records and suppresses unchanged resource inputs.
Inverse property memberships invalidate isolated caches with zero descriptors;
projected checks cover retained content overflow and atomic binding changes.
Fractional padding, surface-only resize, raw rejection/retry, path switches,
producer-independent snapshots and completed mixed-scene controls are verified.

## Current integrated foundation

`gpui-box/retained` owns persistent rectangle nodes and a Taffy tree. Nodes carry
tree identity, slot and generation. Removing a subtree invalidates its handles.
The owner validates property transactions before applying changes, coalesces
field writes and compares final values with committed state. A transaction that
restores the previous value requests no frame.

Color, layout style, translation, clip, scroll and child reordering produce independent update
intents. Style changes enter Taffy through its normal setters. Color and
translation skip layout. Child reorder preserves node identity. Immutable
`FramePacket` values contain owned paint items and shared local command chunks.
Translation and layout reuse existing commands. Hit testing reads the same
published geometry as painting. Root translation propagates to descendants.

The desktop entry `--retained` opens a GPUI gallery. Its normal click handlers
submit transactions, call `notify` when a commit requests output, and paint the
published frame in the GPUI canvas prepaint/paint lifecycle.

```powershell
mise exec -- cargo run --locked -p gpui-box-example-desktop -- --retained
mise exec -- cargo test --locked -p gpui-box --features retained
```

This adapter declares default backend capabilities and uses ordinary GPUI paint.
The earlier WGPU single-quad upload prototype remains a separate experiment.
`InstancePatch` is a routing intent for a consumer declaring that capability.
The normal canvas adapter currently selects `RepaintChunks` for color changes.
With the `retained-wgpu` example feature, the same click and prepaint flow also
submits the frame to a persistent offscreen GPU consumer. The explicit hardware
click test passed pixel comparison at the actual window scale.

## Windows native GPU surface stage

The normal retained WGPU gallery now selects a DX12 device matching the native
D3D11 adapter LUID and paints shared immutable GPU snapshots in the GPUI scene.
The completion-safe pool checks exclusive snapshot ownership plus producer and
native read fences. It preserves cached/old frames, changed extents and explicit
reset. The normal `submit_for_window` path also detects a replaced platform D3D11
identity, advances a consumer epoch, clears GPU-side caches/arenas and replays the
complete owned frame. A matching-DX12 failure selects the existing readback
bridge. Output export avoids CPU readback and sprite upload; it still copies the
complete retained target on the GPU. The original readback mode remains an
explicit control and now handles premultiplied alpha correctly.

See [native qualification](RETAINED_NATIVE_SURFACE.md) and
[the interoperability decision](ADR_RETAINED_NATIVE_SURFACE.md). This stage does
not close visible OS presentation latency, native damage/buffer age, a
driver-induced TDR/physical device removal or cross-platform acceptance.

## Full-goal acceptance tracking

The experimental [local snapshot route](ADR_RETAINED_LOCAL_SNAPSHOT.md) now
publishes persistent local placement overrides and feeds the normal GPU canvas
without resolving every moved descendant. A private predecessor identity
authorizes spatial updates; depth-32 anchors keep shader traversal bounded.
Skipped bases, unsupported capabilities, opacity/layout/material changes and
device/surface recovery use complete resolution. The resolved path remains a
control and vector bridge. Local hit geometry is exact but linear; damage and
isolated layer extents are currently full viewport. Those costs remain in the
performance evidence and require local-space indexing/cache follow-up.
The [two-run local evidence](performance/2026-11-12-local-snapshot/README.md)
shows 80–93% ancestor completed-work median reductions against the complete
route, while 120 Hz misses and small-scene tail regressions remain. Dense
independent updates still exceed 60/120 Hz. This is not visible presentation
or modern desktop GPU acceptance.

The next [dense-page refinement](performance/2026-11-12-dense-pages/README.md)
replaces private local AVL overrides with immutable indexed pages and uses a
contiguous projected mirror for full GPU spatial validation. Alternating
previous/current binaries show 28–32% dense completed-work median improvement,
including losing ancestor-only cases. It preserves generation/snapshot
ownership and backend rejection/retry checks. This does not close the remaining
dense 60/120 Hz, effect raster reuse, native visible or software acceptance.

The [compact transport refinement](performance/2026-11-12-compact-transport/README.md)
removes PaintItem/resource cloning from local spatial updates and uses touched
dirty-bit words for arena uploads. Exact binding lookup hashes identities while
ordered source memberships preserve dispatch. Alternating binaries show 16–19%
dense submission improvement but only 0.4–5.7% completed median improvement;
commit/publication and ancestor/effects regressions remain in the report.
Two sustained runs still fail dense 60/120 Hz: independent/effects 120 Hz p99 is
39.14–39.27 ms, with 1,199 consecutive unsatisfied ticks. Current-source checks
pass 147 core / 11 example tests and 2,011 hardware scenarios / 1,921 pixel
comparisons, plus hidden native delivery/surface/device replacement. This is
partial internal transport work; modern desktop visible 120 fps, software
rendering and the full RFC remain unaccepted.

The [grouped preflight refinement](performance/2026-11-12-grouped-preflight/README.md)
now coalesces and validates one sorted node at a time while retaining the whole
final change list until atomic publication. It avoids the extra dense Pending
array without bypassing overwritten-invalid input checks or last-writer rules.
Alternating exact binaries reduce dense commit medians 21.6–25.2% and completed
medians 6.9–14.5%; an ancestor/effects median regresses 30.6%, kept in evidence.
Two sustained runs produce dense/effects 120 Hz p99 of 29.55–34.33 ms, still
1,199 consecutive unsatisfied ticks, and dense 60 Hz also fails. Hardware checks
again pass 2,011 scenarios / 1,921 pixel comparisons. This does not close visible
presentation, modern desktop hardware or software rendering acceptance.

The [visual admission stage](performance/2026-11-12-visual-admission/README.md)
adds nonblocking owner capacity observation before replaceable source fanout,
normal gallery inbox gating via existing completion wakes, epoch resubscription,
and timestamped covering completion receipts while retaining serial-only awaits.
A pipelined 60/120 Hz harness compares admission before/after construction and
preserves source skips, upper-bound misses, owner receipt delay and flight counts.
Two ordinary 800x600 runs have zero gate waits and show no stable gate benefit;
dense/effects 120 Hz p99 remains 30.08–33.68 ms with 1,199 consecutive unsatisfied
ticks. Owner receipt handling itself can lag another 17–21 ms at p99. These
measurements strengthen the need for bounded owner work and local-space raster
reuse; they do not close visible presentation, CPU rendering or 120 fps.
The one-flight 2400x1800 control records sixteen capacity waits across both
runs, including a losing before-work effects/120 Hz case with an eleven-tick
burst. Final checks pass 150 core / 11 example tests, 2,011 hardware scenarios /
1,921 pixel comparisons, hidden native source delivery and shared surface/device
replacement. Deterministic normal-host saturation, sustained visible presents,
burst/churn and CPU fidelity/responsiveness remain open.

The latest performance priority is modern desktop GPUs at 120 Hz with effects,
especially p99/max and burst stalls; software rendering prioritizes responsiveness.
The [frame-budget decision](ADR_RETAINED_FRAME_BUDGET.md) records API consequences
and explicitly unimplemented parts. Property-only publication now reuses local
payloads and batches dense persistent-map edits, retaining an original extractor
control. The sustained test uses an independent OS worker, the actual bounded
source inbox, owner wake/computed/typed sink transaction, frame queue and GPU
completion at 60/120 Hz. It counts skipped ticks as unsatisfied work and keeps
raw phase/tail samples. Synchronous completion is not visible-window acceptance.
A Windows present-attempt receipt records native status and call boundaries;
compositor/display completion and native sustained receipt correlation are open.
`RenderPreference`/`SoftwareEffects` are experimental policy declarations, not
an implemented software backend. The total goal remains active. The
[two-run evidence](performance/2026-11-12-dense-cadence/README.md) records real
regressions, a 9.26 ms small-scene spike and failed 10,000-node deadlines;
it does not claim sustained visible 120 fps.

The [slot dirty-journal stage](performance/2026-11-13-dirty-journal/README.md)
removes two ordered-set insertions per changed property using private slot-indexed
membership journals. Alternating binaries reduce dense commit medians 30.2–39.4%
and completed medians 2.2–25.2%, with small-scene regressions retained. Two sustained
runs still fail dense/effects 120 Hz: p99 32.59–33.87 ms, max 41.27–41.68 ms
and 1,199 consecutive unsatisfied ticks. All 152 core / 11 example tests,
2,011 hardware scenarios / 1,921 pixel comparisons and hidden native delivery /
shared-output / device-replacement checks pass. No visible or modern desktop
120 fps qualification or software renderer is claimed; the goal stays active.

The [dense spatial backend stage](performance/2026-11-13-dense-validation/README.md)
removes dense ordered-map construction and repeated ancestry walks while
preserving raw-record checks, overlapping last writers, unreferenced-prefix
validation and rejected-plan retry. Alternating exact binaries reduce dense
submit medians 7.0–19.7% and completed medians 2.7–15.9%; the second ancestor/effects
case regresses 33.0% in completed median. Current synchronous sustained dense/effects
120 Hz p99 is 38.80 / 28.24 ms, with 1,199 consecutive unsatisfied ticks in each
run. Same-sample owner work alone has 12.33–12.56 ms medians and every non-initial
consumed 120 Hz dense/effects sample exceeds budget before finish. The new analyzer
preserves correlated longest stalls instead of summing phase percentiles.
154 core tests, 11 ordinary example tests and 2,013 hardware scenarios / 1,921
pixel comparisons pass. All 48 pipelined cases preserve bounded flights, final
reference pixels and lease retirement, with zero capacity/resource waits. Dense/effects
120 Hz completion upper-bound p99 remains 27.37–29.36 ms and owner receipt observation
p99 37.83–39.59 ms; extra receipt handling p99 is 13.10–14.21 ms. Both admission
orders retain 1,199 consecutive unsatisfied ticks, including losing cases. Current
hidden native delivery, 97,792 shared pixels and 3,072 recovery pixels pass.
Placement-independent effects now have a proposed content/placement/raster-environment
contract, including fractional sampling/clip/resource-epoch limits; implementation,
visible modern desktop qualification and actual CPU rendering remain open.

The [dense invalidation study](performance/2026-11-13-dense-invalidation/README.md)
rejects three generic dependency-closure candidates after exact alternating
measurements: unconditional gather/sort regresses ancestor submission 15.7–35.1%;
selective gather/sort has no stable dense/effects benefit; contiguous selection
regresses dense/effects submission 23.0/30.3%. Full raw captures and changed-source
ZIPs remain archived. Runtime restores `634c8ba`, preserving the original dense
ancestry invalidation control and prior sustained baseline. Added isolated Quad
and image/glyph dependency regression cases remain. These results do not close
owner work budgets, effects raster reuse, modern desktop visible 120 Hz or CPU
rendering acceptance. The overall goal stays active.

The [experimental CPU consumer](ADR_RETAINED_SOFTWARE_CONSUMER.md) now renders
actual CPU pixels from owned packets using resumable tile work and nested isolated
opacity, without GPU readback. Completed images retain their hit-test packet;
partial/failed/cancelled jobs cannot publish. Exact shadows are explicitly
unsupported; decorative omission requires application permission. Initial pixel
controls and synthetic probe/cadence measurements are recorded separately from
native input/presentation. Fractional edge discrepancies, native fallback wiring,
resource-pressure tails and competitive software throughput remain open. This
advances CPU implementation beyond policy enums without closing acceptance.

The [integer layer translation probe](ADR_RETAINED_LAYER_TRANSLATION.md) adds a
default-disabled backend equivalence check for bounded solid Quad/Shadow layers.
Two independent ten-second resolved-route overdraw captures reduce 120 Hz
completion p99 from 15.07–16.48 ms to 8.39–8.70 ms, max from 25.94–30.80 ms to
10.50–11.00 ms, and longest unsatisfied bursts from 7/33 to 2/2. It still misses
13/15 of 1,200 source deadlines; no stable 120 fps or native display claim.
Complete proof rebuilding has CPU/submission cost. Resource/nested/fractional
fallback, broader local-space content epochs, dense independent updates and
modern desktop visible qualification remain open. See the complete
[evidence and scope](performance/2026-11-13-layer-translation/README.md).

The [local layer bounds integration](ADR_RETAINED_LOCAL_LAYER_BOUNDS.md) enables
current tight ink and integer raster reuse on the private local snapshot route,
with complete recovery, mixed-resource/fractional/deep-anchor pixels and exact hit
geometry. It is independently default-off: exploratory alternating 10,000-node
ancestor/effects submit medians regress 174.8/266.5%, and dense-independent/effects
regress 76.5/63.4%. Targeted local-overdraw 120 Hz p99 improves to 8.01/10.66 ms,
but still misses 10/36 deadlines of 1,200 and 60 Hz optimized maxima can worsen.
The normal local route retains full-viewport layers. Persistent local-content ink
and separate placement/clip proof remain required; smaller textures alone do not
solve dense owner costs. Full [losing and winning evidence](performance/2026-11-13-local-layer-bounds/README.md)
is preserved, and the overall goal stays active.

The [private lazy ink proof](ADR_RETAINED_INK_TRANSLATION.md) removes descendant
ink resolution for a qualified integer whole-subtree translation, using immutable
predecessor provenance, conservative uncut geometry, clip containment and lazy
segment offsets. The outer tight-local feature remains default off. Alternating
10,000-leaf integer ancestor/effects submit medians improve 56.1/69.8% and
completed medians improve 60.7/51.5%; dense-independent submit still loses
4.2/7.7%, with a 9.2% losing completed median. Sustained 120 Hz source losses
fall from 64/334 to 27/30, but first optimized p99/max worsen to 14.86/30.70 ms.
Full raw [evidence](performance/2026-11-13-ink-motion/README.md), fallback controls
and failures are retained. This reduces routine bounds work without closing
fractional/resource generality, burst stalls, visible 120 Hz or full acceptance.

The [burst recovery follow-up](ADR_RETAINED_BURST_RECOVERY.md) adds sustained
60/120 Hz ancestor/effects animation with persistent 10,000-leaf color changes
every half second. Full local publication now shares the already resolved owner
snapshot for recovery, avoiding duplicate geometry/property reconstruction.
Alternating binaries reduce dense submission medians 18–26%, but 120 Hz p99 stays
34.29–37.47 ms and the first pair loses more deadlines (134 vs105). Full normalized
raster comparison also loses in the burst study. Material changes still reset
local topology/transition continuity; separating owned material/placement/topology
is the next API priority. Modern desktop visible120fps and smooth native CPU
fallback remain open. Raw winning/losing runs, hashes and process observations:
[evidence](performance/2026-11-13-burst-recovery/README.md).

The [independent material/placement stage](ADR_RETAINED_MATERIAL_PLACEMENT.md)
patches local material pages and journals without resetting render topology or
extracting unchanged descendants. Existing shared material refresh, consumer
classifiers and GPU validation handle color/decorations/shadow parameters,
same-size image pixels and text color. Changed draw topology, opacity, layout,
missing bases and device changes preserve complete recovery. Initial reversed
60/120Hz burst captures reduce publication medians13ms ->5–6ms and submit15ms
->10–11ms;120Hz p99 remains22ms and the second longest unsatisfied interval
worsens5 ->6. This advances the general dependency design without integer/scene
special cases; it does not close120fps, native display or CPU fallback acceptance.
Final-source reversed pairs retain120Hz p99 reductions37.04/45.52ms ->22.56/23.05ms
and longest unsatisfied intervals7/11 ->3/4. First60Hz longest interval regresses
2 ->3.164core,11example,72mixed GPU and2,015hardware /1,923pixel cases pass.
See [full evidence](performance/2026-11-13-material-placement/README.md).

The [material streaming follow-up](performance/2026-11-13-material-stream/README.md)
removes the full resolved changed-item temporary array. Material is validated,
classified and prepared in its visitor lifetime; only resource changes retain
owned items for batching. Late validation failure submits nothing, preserves old
pixels and requires reset.164core,75mixed GPU and2,018hardware /1,926pixel cases
pass.120Hz submit medians improve, but first60Hz regresses and first120Hz longest
unsatisfied interval worsens9 ->14. This reduces transient work without a stable
stall-elimination claim. The full goal stays active.

The [pipeline completion follow-up](performance/2026-11-13-pipeline-completion/README.md)
adds10second60/120Hz pipelined burst controls with completion/owner times kept
separate. Default harness waiting now handles completion independently of source;
normal native integration already has an independent completion task.16eligible
cases /14,400ticks preserve final pixels, bounded flights and complete retirement.
Idle-source completion regression, strict harness/test Clippy and32final release
smoke cases pass.120Hz handling p99 improves to0.50–1.14ms, but completion p99
stays22.56–24.77ms; three of four completion-loss comparisons worsen. Dense
owner work remains20.10–22.03ms median. This corrects the harness and clarifies
the host API contract; it does not close burst stalls or justify queue tuning.
Rejected interference runs and raw regressions are preserved. Full goal active.

The [publication-order follow-up](performance/2026-11-13-publication-order/README.md)
sorts topology indices before material construction, removing wide-record and
duplicate index sorts. Synchronous dense publication medians improve9–15%;
pipelined medians improve7/8cases, with one60Hz regression.24formal cases retain
raw deadline/tail losses and no known compiler interference.165core tests and
75mixed GPU comparisons pass; strict library Clippy passes, test Clippy exposes
a pre-existing local_ink test-module-order lint. No API/quality switch is added;
dense owner work and full modern desktop120Hz acceptance remain open.

The [direct arena conversion probe](performance/2026-11-13-arena-conversion/README.md)
removes temporary source-record arrays and sizes converted GPU runs directly.
It passes166candidate core tests and75mixed GPU comparisons, but reversed16case
pipeline captures show4submit medians improving /4regressing (about±2%). Dense
owner work remains near20ms and tails/deadlines remain mixed. The candidate is
rejected; runtime sources are restored tob956e30 with exact candidate and raw
evidence preserved. Generic baseline/label harness controls remain. Next priority
is internal submission phase attribution before choosing a new algorithm.

The [submission phase study](performance/2026-11-13-submit-phases/README.md)
adds compile-time-only owner timing and same-row nesting/residual checks.16
reversed60/120Hz cases attribute dense120Hz preparation4.41–4.60ms, batch
construction0.58–0.67ms, backend2.75–2.93ms and queue submit0.40–0.42ms;
post-body outer residual0.91–1.00ms remains unattributed. Timing perturbation and
regressions are preserved; these are diagnostic spans, not GPU execution or
speedup evidence.165core/2harness tests,75mixed and2,018hardware /1,926pixel
comparisons pass. Default builds compile out the phase fields/clock reads.
Next focus is repeated preparation and explicit temporary destruction, while
native display/input, CPU integration and full acceptance remain open.

The [identity-map experiment](performance/2026-11-13-instance-lookup/README.md)
passes165core and75mixed GPU comparisons, but all eight reversed sustained
dense submit medians regress2.4–14.0%. The candidate is rejected and the original
runtime maps restored exactly. Raw tail/deadline losses, source hashes and the
candidate are archived. Bulk preparation/cleanup attribution is the next step;
no new rendering policy or application API is introduced.

The [preparation/cleanup study](performance/2026-11-13-preparation-phases/README.md)
adds bulk diagnostic-only subspans. Dense120Hz material preparation is4.49–4.90ms;
spatial application and layer records are each about0.002ms in this workload,
which leaves tight bounds disabled. Explicit temporary destruction as a group
accounts for0.92–1.04ms, leaving about0.002ms unattributed; no individual object
or allocator cause is established.16reversed cases preserve tail regressions
and all pixel/lease/base assertions. No runtime speedup or off-thread destruction
policy is claimed. Material profiling and native/CPU acceptance remain open.

Material-path investigation exposed a geometry-range correctness defect: a visit
starting inside a deep subtree reused its starting parent's context after
crossing to a shallower sibling. The visitor now retains all open ancestors and
pops to each actual parent, preserving transform/clip/effect inheritance. A
regression compares every possible range start against fresh extraction across
deep motion/clip/opacity and siblings. This is an independent correctness fix;
future performance comparisons use the corrected baseline.

The [borrowed material experiment](performance/2026-11-13-borrowed-material/README.md)
then removes transient shared-payload cloning through an internal geometry view.
167candidate core tests/75mixed GPU checks pass, but16reversed sustained cases
show6submit median gains/2regressions and all four120Hz completion-loss totals
worsen, including a4→13tick longest interval. Reject the candidate and preserve
the independent ancestry fix. Raw evidence and exact sources are archived;
no extra representation or application policy remains in the runtime.

The [sparse callback study](performance/2026-11-13-material-sampling/README.md)
samples one in64material indices with a rotating revision residue. Across16
reversed sustained cases, sampled primitive preparation and spatial lookup are
larger than validation/classification/content; clock overhead prevents exact
whole-frame extrapolation.166core/2harness/75mixed GPU checks pass. Raw p95/p99/
deadline regressions are retained. Next investigate consumer-owned allocation
indexing tied to exact submitted topology, with generation checks and full
recovery; this adds no application-selected algorithm or deadline promise.

The [consumer slot index](performance/2026-11-13-local-slot-index/README.md)
removes repeated material-node spatial lookup using private paint-order metadata
rebuilt after complete recovery. Eight reversed dense submit medians improve
7.5–19.8%; keep this small general reduction.120Hz p99 improves but longest
after-work intervals regress4→23 and3→8ticks; dense owner remains18–20ms.
Tail/deadline failures are preserved and120Hz qualification remains open.
Generation checks, exact predecessor authority and complete recovery are intact;
42additional hardware pixel cases cover reorder/removal/missed-base/reset/handoff.

The [consumer instance index](performance/2026-11-13-local-instance-index/README.md)
also retains actual Quad/Shadow handles under the same exact topology/allocation
lifetime, preserving arena generation and retirement validation. Its validated
reversed repeat improves all eight dense submit medians4.0–18.8%, with32bytes/
item plus24bytes/handle logical metadata on64-bit. The initial capture's single
1microsecond timing-order anomaly is preserved and rejected for full-chain
analysis. Two repeat120Hz p99 pairs and one deadline total worsen; dense owner
still16.85–18.36ms. This is a general internal reduction, without a new API or
scene-specific mode. Smooth120Hz and complete native/CPU/RFC acceptance stay open.

The [page refresh follow-up](performance/2026-11-13-page-refresh/README.md)
refreshes local material in immutable page order with at most64transient payloads,
removing the full wide patch array while retaining complete publication and old
snapshot ownership. Eight reversed publication medians improve37.3–51.6%, and
all completion p99 pairs improve. Mixed120Hz p95/max and a worse loss/streak
45→60 /2→6remain; dense owner still15.01–15.78ms. This general producer change
does not add an API policy or close native/CPU/modern desktop acceptance.

The [preflight reservation probe](performance/2026-11-13-preflight-reserve/README.md)
is rejected: exact node-group capacity improves six commit medians and regresses
two, while all four120Hz p95 comparisons and both round2loss totals worsen.
The runtime is restored byte-for-byte and release binaries rebuilt.167core and
117mixed GPU checks pass for the candidate; no new transaction/API policy is kept.

The [transaction attribution study](performance/2026-11-13-transaction-phases/README.md)
adds compile-time-only source/sink/tree/publication and nested property timings.
Strict reversed sustained analysis identifies sink collection1.87–2.15ms,
preflight1.45–1.82ms and application1.07–1.28ms at dense120Hz; source settlement
is0.024–0.028ms in this shared-source fixture. Diagnostic observer regressions
remain documented; default clocks/fields compile out. This is attribution, not
a renderer speedup or closure of native/CPU/modern desktop acceptance.

The [ordered-binding probe](performance/2026-11-13-ordered-bindings/README.md)
is rejected: commit medians regress six of eight pairs, and all120Hz p99/loss
totals worsen with after-work streaks up to14ticks. The original tree-backed
source membership is restored; no source/sink API policy is added.

The [binding-slot probe](performance/2026-11-13-binding-slots/README.md) is
rejected despite lower commit medians: 120Hz after-work longest unsatisfied
intervals grow to15/16ticks and loss totals worsen. The runtime is restored to
the original checked binding table; no source/sink API policy is added.

| Requested area | Current implementation | Remaining acceptance work |
| --- | --- | --- |
| Exact custom measurements | Full-input/full-output nine-entry sparse per-node stores; Arc copy-on-write for private previews, complete percentage/baseline metadata and exact-only dirty propagation; real Flex/paragraph/resource and old-frame verification | Global byte budgets, dependency-indexed invocation selection/retirement, whole-tree/private-map clone costs and timing qualification |
| Atomic keyed frame output | `LogicalUi::edit_frame` and `edit_local_frame` share source/keyed lifetime/final layout/builder observation validation; direct sparse local output avoids resolved item/property-map construction; non-resolving published local queries; source/effect/geometry rollback, real glyph CPU and consecutive/skipped-base GPU full-reference equality | Whole-tree private candidates and per-builder previews remain; final local geometry validation is O(N); incremental planning/rollback, combined query structural plans, automatic scheduling and end-to-end performance acceptance |
| Normal UI update chain | Desktop click → signal → typed property sink → transaction → layout → FramePacket → persistent WGPU; Windows shared GPU snapshots in the native D3D11 scene, preserving normal UI ordering/clipping; the normal `submit_for_window` path detects native device replacement, advances a new consumer epoch and restores from a complete frame, with matching-DX12 construction falling back to the readback bridge | Native visible presentation qualification, driver-induced TDR/device-removed qualification, other platforms and complete general routing |
| P0 equality and transactions | Source equality before fanout, final-value coalescing, explicit computed DAG with ordered propagation/equality cutoff, transaction-consistent reads, atomic source/derived/sink validation, nested savepoints, keyed dependency lifetime checks, synchronous post-commit effects with deduplication and replacement/scope/drop cleanup; bounded background last-value source inbox with executor wake, owner drain and stale-source cleanup; one outer frame intent, real normal-click/worker computed/effect route and idle-render assertions | General asynchronous/reentrant effect scheduling, dynamic dependency tracking, dense graph scheduling costs, generalized producer priority/backpressure/byte budgets and visible-window automatic wake/OS idle-wakeup/energy measurements |
| P0–P5 routing | Solid/decorated-quad color, immutable rounded-box shadow paint with typed shared-list sinks, property, layout, reorder and decoded-image resource intents; reason records; P1 reuses commands with 0 layout/recording and stable instance dispatch; indexed producer patches and immutable-page consumer validation reuse | General primitive/material capabilities, semantic channels, general asynchronous resource invalidation and eliminate remaining full resource/order scanning |
| Logical / Render Tree | Independent generation-checked scopes, zero/multiple outputs and portals, atomic existing-node topology/removal batches and mixed-key reconciliation, multi-host keyed reorders, portal reparenting retaining scope/state/render/chunks, unified sparse render construction/topology/property edits with existing source/computed writes and binding construction/unbinding/replacement plus keyed scope/state/computed/effect construction and retirement in one validated revision, final-cycle/ownership/leaf/revision preflight, provisional ID safety, subtree source/binding/output cleanup; gallery and hardware validation | Dynamic dependency/lifetime changes during construction, asynchronous effects, event/focus/semantics lifetime and legacy islands; eliminate whole-scope/order scans and full topology validation/extraction |
| Persistent Taffy | Long-lived nodes, style/children setters, owned image/fixed/shaped paragraph snapshots and generic pure ContentMeasureSnapshot leaves through typed measure properties; selected-inline dependent-height remeasurement, finite signed baselines/natural overflow, complete-input cache capability with node lifetime, isolated fallible layout and explicit queries; actual leaf/measure-call and layout-write/net-change counts, root cache and resize handling; optional observed output writes from pinned vendored Taffy | Complete RenderBox/intrinsic/custom-input semantics, paragraph conformance, independent islands, full node visits/cache-hit counters, cache byte budgets and private candidate costs |
| Paint chunks / property trees | Shared local fill commands and immutable shadow bundles; immutable 64-item paint pages with owned complete-page replacement, sparse partial-page copying, authoritative indexed delta views and original selective-copy controls; selective local material and disjoint property-subtree extraction, indexed node patches/subtree ranges; persistent ordered page-bounds hit index with selective path updates; immutable cached glyph geometry and base-relative backend updates preserving unmodified draws; persistent sparse transform/clip/scroll/effect maps with inherited state, constant-time snapshot cloning and path copying; bulk construction for full extraction; actual-layout-output selective geometry/subtree extraction with stable visibility/order | General stable chunk IDs/store, full affine transforms, incremental visibility/structure/viewport extraction, tighter/adaptive spatial indexing and general hit geometry |
| GPU arena | Generic CPU allocation table with generations and deferred slot reuse; shared typed WGPU Quad/Shadow/112-byte spatial-node mirrors (96 before overflow viewports), geometric growth, range uploads and completion-held replacement buffer/binding leases, including actual overlapping-submission growth checks | General resource/material records, budgets, compaction and other backends |
| Batch GPU updates | Adjacent upload merging; 112-byte ancestor placement patches (historical controls used96) and bounded deep-anchor patches with unchanged Quad/Shadow geometry; 10,000-object translation and 10,000-signal color pixel verification; CPU per-signal/coalesced/typed-batch comparison; completed-work redraw A/B | Sustained tests now cover ancestor and dense independent updates with raw tail/deadline evidence; remaining work is meeting 120Hz, pipelined native visible qualification, general fragmented order throughput and tuning |
| P2 property / composite | Translation/clip/scroll reuse commands and resolved hit/damage geometry; default Quad/Shadow path shares hierarchical 112-byte GPU placement slots (96 before overflow viewports), bounded deep anchors and local primitive geometry with property-driven raster invalidation, stable slot references and correct reparent/removal/reuse; backend-owned indexed shader-parent/finite validation, inverse Quad/Shadow users, transactional preflight and prefix reexposure checks; image and glyph draws share the same hierarchy through ordered content offsets and existing 48/32-byte records, with exact-base input routing, inverse dependencies and retained-bound checks; isolated subtree opacity supports nesting and 16-byte property-only updates | Full affine/complex clips, adoption across other primitive types, remaining resource/order/layer dispatch, other-platform texture import, native presentation/damage qualification and NativeView geometry |
| Text / images / complex primitives | Solid/decorated quads and rounded-box drop/inset shadows with ordinary typed clicks, clipped ink and persistent 112-byte records; immutable decoded RGBA textures, base-relative image draw updates and reference-counted shared texture ownership; owned uniform-font LTR paragraphs and grayscale glyph masks, four-width chunks, system/font/scale identity, packed R8 pages, persistent 48-byte local glyph ranges, independent 16-byte color / 32-byte local or world property slots, shared content references and adjacent batch merging; typed normal clicks, clip/opacity, CPU blend and lifecycle pixels | Arbitrary paths/general-shape shadows/filters, automatic font subscriptions, robust glyph pressure/compaction, general/rich-text materials; bidi/rich-text/selection/color/subpixel text; image decoding/fit/filter/color/animation/atlas/batching and physical resource budgets |
| Flex / responsive contract | Engine-independent LayoutStyle/Constraints/Result, validated Flex lowering/order/align-self/overflow/gaps; public numeric Grid tracks/minmax/fr/percent/intrinsic/fit-content, Count/AutoFit/AutoFill, implicit patterns/dense flow/signed-line/span placement, inline item alignment, optional shared track snapshots, Grid/Flex typed/keyed atomic composition, safe external-host builder/fixed-or-contained query integration and 1,152 independent Chromium geometry matches with a corrected kernel padding/border contribution floor; owned linear px+percentage Calc/FitContentCalc in public lengths/dimensions/Grid tracks, mixed Row definite-chain proof, preserved custom-style inputs, 336 browser cases and actual glyph/numeric controls, with optional kernel inline coefficients and documented 200-byte Style growth; public Alignment<T> with logical/flex/self keywords and independent Safe/Unsafe policy, atomic source/no-op validation, reverse Stretch kernel corrections, 5,364 Chromium145 geometry matches plus twelve separately asserted spec/browser SpaceBetween differences, real glyph and independently positioned CPU/WGPU controls; preferred and min/max intrinsic/FitContent/Stretch sizes, Content flex-basis, final-width Flex cross-limit measurement, provenance-aware automatic minima for replaced/ordinary content, BorderBox/ContentBox ratio transfer and final automatic cross sizing, root keyword/percentage-definiteness fixes in the project-owned Taffy fork; isolated intrinsic queries and published first/last horizontal baseline sets, independent first/last Flex sharing groups and public LastBaseline item/self alignment, final-width shaped paragraphs and dual-metric custom parents; owned pure custom parent measurement/constraint/placement snapshots with read-only unresolved direct-child item styles in measured phases, composed with real nested Flex/text, baseline/overflow and local/resolved publication, optional enforced width-only ParentInlineLayout phases integrated with query/builder proofs, algorithm/ordered-child observations and sibling-order query stages; independent incoming full-axis ParentAllocationLayout intervals with read-only unresolved child item styles and conservative effective sibling-style observations, enforced across measurements/final placements and allocation_builder content-sized construction along external-root independent chains, complete-input final cache guards and clone/hidden/node lifetimes; mixed ordinary-Flex ancestry across explicit full Size+Layout cuts and size-contained ConstraintBox outgoing slots, real incoming known dimensions/percentage definiteness distinct from hard intervals/soft space/bases, explicit Inherent/Content mode for child queries/placements and parent/full/inline/builder inputs with own-style keyword bypass and descendant-style preservation, exact contained final-input records and cache/visibility/node lifecycle; normal Unbounded distinct from MaxContent; explicit public scrollbar_width, actual scrollbar_size and gutter-deflated content_size shared by Flex/parent slots/builders/container queries/paragraph baseline/glyph/image output, RTL content origins and normal geometry invalidation with 288 Chromium145 matches; hard BoxConstraints for layout roots/isolated subtree queries, explicit nested single-child Tight/Loose constraint boxes with padding deflation, selected-width remeasurement and CSS Flex composition, resolved/local frames with an independent finite paint viewport and zero-layout viewport-only edits; Normal/InlineSize/Size boundaries with per-axis explicit intrinsic inner fallbacks shared across leaf/owned boxes and child-style views, fluid Size and proven auto-height Column queries plus ordinary uncontained used-content queries through complete Column/Tight-box/independent-width chains with live external inline anchors, standalone final width/algorithm/order guards, nested DAG/scoped automatic session integration and computed feedback rejection; staged sibling dependencies and safe OOF coordinate publication; explicit Size-contained parent-input ConstraintBuilder and inline-only contained construction with content-driven block size, shared Column/Tight-box/fixed-width used-content proofs; ordinary uncontained incoming inline_allocation_builder with complete Column/Tight-box/independent-width chains plus Row/RowReverse/all-wrap allocation from content-independent numeric item bases/minima/maxima, an external inline anchor, conservative ordered sibling/effective-style observations and responsive sibling-producer DAG edges, actual mode/selected-width/definiteness/bounds/soft-space/basis observations and transaction-scoped kernel records; complete incoming ordinary Grid item allocation across definite/fr explicit and implicit columns with fixed row counts, nested Grid/Row definiteness, Grid Style/order/effective sibling observations, sibling query DAG edges and 1,152 additional Chromium geometry matches; complete incoming ordinary-Flex percentage Row allocation proof separates definiteness from content independence under live external anchors, including percentage basis/Auto+percentage width/explicit percentage limits, nested builders, automatic source/query settlement and 432 additional Chromium145 geometry matches; nested current-draft reads, conditional keyed branches and final source/logical/layout/frame atomicity plus finite geometry and final input checks; atomic multi-stage style/source failure recovery, public local responsive output, exact writer ownership and computed layout sink dependency checks; explicit existing-rule viewport/container stages inside keyed resolved/local edits with live/draft computed settlement, final observation/proof checks and per-call source rollback; scope-owned keyed rule declarations/updates/removal with provisional sources/nodes, stable writer identity, automatic scope retirement and final registry validation; opt-in atomically captured environment/constraints/paint sessions, automatic registered-rule settlement for ordinary source/inbox/legacy structural/frame updates and before builder observations, final stale-input validation and complete local output before effects; optional captured-scale absolute-edge presentation projection after logical layout/text wrapping, atomic policy edits and matching CPU/WGPU/hit geometry; native shaped paragraph/CPU clip regressions and independent browser references: 24 preferred sizing + 23 containment + 47 limits (two explicit height deviations excluded) + 14 normal block-axis assertions; responsive local/full CPU and WGPU pixel equality | Broad Flex/intrinsic/baseline conformance, percentage/cyclic sizing and Grid/Block cross-axis coverage, baseline content distribution/vertical writing and broad device-rounding/precision conformance; incremental pixel projection preserving sparse routes and automatic host DPI/zoom integration; remembered intrinsic sizes/content skipping and automatic scrollbar prediction, named Grid lines/areas/subgrid and complex calc functions/units, native OS environment/font subscriptions, ordinary content-feedback/full-axis builder proofs without a containment cut, complete RenderBox and general child-style/custom-policy/query dependency contracts including general intrinsic/automatic Grid item allocation proofs, measurement-cache byte budgets/timing, automatic cross-builder/query scheduling and general dynamic declaration dependency changes; general content/ratio/intrinsic Row, custom/box-protocol percentage, cyclic sizing and Column-wrap/aspect inline query proofs; cached/indexed dispatch, incremental planning instead of full private tree copies |
| Damage / cache / compositor | Old/new clipped ink/effect damage; preserved offscreen target, scissored clear/replay; Full/Damage/area-threshold Auto; isolated layer content cache and cached/uncached A/B; persistent mixed composition topology/shared layer draw arrays; clipped ink bounds cached per immutable paint page, nested bounded textures with per-target cameras, persistent same-extent textures/buffers/bindings with queue-ordered 48-byte origin updates and raster invalidation, live/allocated texture payload and raster-area counters; configurable unique layer-allocation payload cap across live cache/in-flight epochs, pressure wait/retirement, observational resident/retired/peak counters | Remaining layer, resource-ID, budget and lease scans, driver/other-resource/global device-epoch physical memory accounting, cache admission/LRU, measured cost model, multiple damage regions, native surface buffer-age contract |
| FramePacket / frames in flight | Owned resolved/local-placement snapshots and ordered content offsets with page-sharing indexed base deltas, exact-snapshot agreement checks, private normal-producer transition provenance, untrusted/missing-delta full recovery and conflicting-same-revision rejection, plus immutable image resources, skip-frame resync, stale rejection, single-pending producer coalescing and wake intent, bounded GPU queue, native completion worker/host wake with no idle timer, owner snapshot/texture/binding retirement without another frame, subscription cancellation/reset/resubscription, poll/finish/reset with resource reupload; Windows shared output snapshots with producer/native read fences and completion-safe bounded allocation reuse; platform D3D11 identity replacement advances a full-resync consumer epoch while old immutable snapshots retain their original allocation ownership | Driver-induced TDR/physical device removal, general resource/font epochs, native visible presentation and other platforms, cross-platform idle/energy qualification and deferring superseded snapshot production |

## Verification and present limits

Core regression cases cover final-value equality, batch rejection without partial
application, color/translation route coexistence, shared chunks, inherited
translation hit testing, persistent-vs-fresh Flex geometry, zero-size viewport,
subtree removal, slot reuse, invalid viewport/translation and stable child reorder.

The resolved compatibility path for local material/paint publication extracts dirty visible nodes and copies affected
64-item pages plus the N/64 page directory. Resolved visual properties extract disjoint
dirty subtrees and copy only modified paths in persistent property maps. The
experimental local snapshot route instead retains unchanged descendants and
publishes sparse local placement overrides for stable property-only frames.
Stable-visibility/order layout updates observe actual Taffy output writes and
extract only changed items or moved/scrolled subtrees. Structural/visibility/order
and viewport changes still walk all live nodes and rebuild indices. Commands and unchanged pages are
shared; unchanged input reuses the complete CPU frame. Consumer item validation
reuses stable immutable pages; changes to the effect map still check full effect references,
and composition topology reuses unchanged pages/layer arrays. Backend resource-ID,
layer invalidation and completion-lease scans remain before a sublinear complete
scene-update claim.

The initial style surface exposes the pinned Taffy Style for compatibility.
The framework now owns a separate Flex style/constraints/result API, documented
in [the runtime architecture](UI_RUNTIME_ARCHITECTURE.md) and the baseline
contract above. Broad conformance and the remaining CSS subset are still open.
Geometry currently contains
floating-point border bounds, translation, rectangular clip, scroll and isolated opacity. Resources and
accessibility are tracked above for their subsequent implementation stages.

The progress table uses implementation and verification evidence. The active goal
remains open until the complete requested flow and corresponding acceptance work
are finished.

## Foundation validation

- Eight core tests passed with `cargo test -p gpui-box --features retained`.
- The GPUI `normal_click` integration test passed. It locates the rendered buttons,
  dispatches mouse clicks, runs the event loop and checks the actual published
  frame. Changing color changes the display commands. A temporary color restored
  within one transaction retains the frame revision and shared paint-item array.
- The desktop executable built successfully. Core and shared-example Clippy
  checks run with `-D warnings`.
- The existing CLI and Android host lifecycle tests passed, three cases total.

The GPUI integration test uses `gpui/test-support`, which adds its upstream test
and benchmark dependency closure to Cargo.lock. Native screenshot verification
and cross-device qualification remain in the acceptance table.
