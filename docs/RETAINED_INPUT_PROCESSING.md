# Atomic component input processing

Status: experimental explicit Rust owner API, 2026-11-18. Extends
[scoped events](RETAINED_SCOPED_EVENTS.md),
[style feedback](RETAINED_LOGICAL_STYLE_FEEDBACK.md) and
[default scrolling](RETAINED_LOGICAL_SCROLL_DEFAULTS.md).
Complete RFC remains active.

[Event task declarations](ADR_RETAINED_EVENT_TASKS.md) now join callback source
writes in the same owner transaction. EventContext exposes scoped start/task/
cancel without render construction; launches wait for complete adoption/effects.
Failure or same-candidate retirement prevents launch and retains old task state.

The [GPUI owner host](RETAINED_GPUI_OWNER_HOST.md) now demonstrates explicit native
event normalization, host focus, complete CPU/GPU image installation and matching
scene adoption. The owner API itself continues to infer no native presentation.

Source-owned scroll controls now share `UiScrollState::consume` and
`EventContext::remaining_wheel_delta`/`scroll_wheel`. Inner handlers consume from
current transaction values; residual distance continues to ancestor callbacks and
defaults. A default after controlled consumption resumes at the original handling
ancestor, allowing a virtual row to retire without losing outer scroll chaining.
The final anchor/route must still survive eligible. Standalone dispatch_event has
no remaining delta/defaults. See [virtual lists](RETAINED_VIRTUAL_LIST.md) and
[decision and limits](ADR_RETAINED_VIRTUAL_LIST.md).

## Owner entry and host evidence

`LogicalUi::handle_input(router, input)` routes one physical input against the
router's explicitly installed scene. It clones router state, validates owner
identity, dispatches callbacks and adopts callback source writes, style feedback,
component/layout producer output and default actions in one complete candidate.
On validation failure the live router, sources and frame remain unchanged.

`LogicalInputCommit` returns the ordinary reactive commit, delivered/ignored/
stopped/prevented counts, per-wheel consumed/remaining distances and complete
immutable LocalFrame. It does not install pixels or silently advance the input
scene to the newest publication. Subsequent input remains targeted against the
last image the host adopted; the current owner revalidates those routed identities.

```rust,ignore
let result = ui.handle_input(&mut router, input)?;
host.install(&result.frame)?; // application's actual render/image installation
let scene = ui.input_scene()?;
let adoption = ui.adopt_input_scene(
    &mut router, scene, UiInputEvidence::ImageInstalled,
)?;
// Adoption can produce enter/leave/cancel callbacks and change appearance.
// Install adoption.frame subsequently if its commit requests a new visual frame.
```

`adopt_input_scene` processes the router's adoption callbacks/feedback through the
same candidate and adopts its router draft only on success. The caller must name
the scene matching the image actually installed, not merely call this after an
offscreen renderer finishes. No recursive adopt-until-stable loop is installed;
hover styles can move hit geometry, so the host schedules subsequent requested
frames normally. Image installation is externally irreversible: if adoption
processing fails, the host retains its installed scene for retry/recovery and
must not claim transactional rollback of the display.

`process_input_effects` accepts one route/adopt/reset result for explicit hosts.
It does not mutate their router and must be called once in router order. A combined
vector of several physical events has no event IDs and is outside this contract.
Use handle_input when router rollback on failure matters.

## Callbacks and propagation

Callback registration, ownership, generation and route acceptance are the existing
scoped event rules. Handlers resolve against the current owner scene once. Routed
events retain the installed target/route; moving alone does not re-hit-test a click
or wheel against an undisplayed image. Retired/foreign/replaced callbacks ignore.
Wheel/default requests receive strict current route validation before any callback
runs; malformed/foreign input cannot execute business code before rejection.

Callbacks execute in router order in one source transaction. A subsequent callback
can read previous staged source writes through EventContext. Producer/structural
settlement runs after this dispatch phase, so the candidate does not remount new
handlers halfway through one physical event. All delivery decisions belong to this
fixed validated dispatch route, followed by one final source/layout publication.
The lower-level dispatch_event continues its single-callback transaction semantics.

`EventContext::stop_propagation` stops remaining ancestor callbacks in this event
group. It does not suppress hover/focus notifications, unrelated event groups or
default actions. Group boundaries are the contiguous router-generated dispatches
with the same scene/target/kind/route/position/wheel delta. Existing enter/leave/
focus notifications remain non-bubbling, and Activate still selects the router's
nearest activation owner.

`UiInputEventKind::Wheel` is now emitted through the existing pointer-handler
policy before default scroll. `UiInputDispatch::wheel_delta` carries normalized
logical pixels for Wheel and is None for other events. This extends public event
types; exhaustive matches and struct literals must be updated. The owned UI source
schema remains 6 because no UiNode/UiBatch source field changed.

## Supported default cancellation

`UiInputDispatch::cancelable()` states whether the combined processor can cancel
a remaining default. `EventContext::prevent_default()` returns false on unsupported
notifications; true records cancellation. Later ancestors see `default_prevented`.
Propagation and cancellation are independent.

| Notification | Cancellable remaining default | State still maintained |
| --- | --- | --- |
| Wheel | Default scroll for the matching target/route/delta | Hover targeting/feedback and bubbling callbacks |
| Focus | Automatic focus reveal | Router focus transfer already happened |
| PointerUp | Synthesized Activate from this router result | PointerUp callbacks and capture release |
| Others | None in this version | Existing router notification/feedback behavior |

Prevented wheels return consumed zero and their original remaining distance. Stop
propagation alone still permits wheel scrolling. Cancelled PointerUp activation
does not run its callback or queue its business action. This is an explicit native
framework subset, not full browser DOM capture/bubble/default-action compatibility.
PointerDown cannot retroactively cancel already routed focus/capture; keyboard
focus traversal, focus/activation editing and platform shortcuts remain future work.

The low-level `dispatch_event` owns no associated default list, so prevent_default
returns false there; use the combined processor for effective cancellation. It also
cannot stop callbacks an application manually dispatches in separate calls.

## Final defaults, rollback and panics

After callbacks, feedback uses existing scoped ReadProp/computed/StylePlan sinks;
producers and Flex/text layout settle normally. Defaults use final offset/range/
geometry and honor application scroll ownership. A formerly valid target that is
removed, reparented, hidden or disabled by that same candidate skips its default;
wheel consumption is zero and residual is returned. This avoids discarding a valid
business/state update because the target intentionally disappeared. The independent
edit_input_scroll API retains strict stale-route rejection.

Handler error/unwind, source propagation, producer, geometry or final validation
failure rolls back all callback writes, feedback, defaults and complete frame.
Deferred after_commit work is discarded. On success the router draft adopts after
UI/logical publication and **before** ordinary reactive effects and after_commit
business work. A post-commit panic cannot undo adopted UI, but it no longer leaves
router capture/focus on pre-event state. Effects before a panic may already have
external consequences; direct handler side effects remain outside rollback.

Old frames remain immutable and consumable after owner disposal. Adopt/reset cleanup
whose old target is no longer live/eligible may still be ignored under current
handler validation; scope/effect disposal remains the cleanup authority.

## Costs and remaining boundaries

Router clone shares immutable scenes but copies owner interaction state. Processing
builds current scene maps and feedback maps, validates defaults twice and resolves
callbacks by scanning the registry. It copies the effects list and performs usual
source/computed/producer/candidate/layout work. This stage claims no FPS, latency,
memory or continuous presentation gain.

This is the common owner input pump, not a native OS window binding. Platform event
normalization, actual image installation/presentation, native focus/capture/IME,
semantics/accessibility, editing/multipointer, event serials/passive/capture phases,
task scopes/KeepAlive, full source unification and independent native presentation
remain open. No Taffy solver/GPU format change. “99%” means common fluent/intuitive
behavior, not a numerical success ratio or p99 gate.

See [verification and reproduction](performance/2026-11-18-input-pump/README.md).
