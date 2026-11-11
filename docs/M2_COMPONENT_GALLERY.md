# M2 component gallery progress

## Implemented slice

`components/gpui-box-components` owns reusable UI. It depends on the single copied
GPUI identity and the shared `gpui-system` error contract. `examples/hello` owns
the consumer gallery. Both the Windows executable and Android cdylib launch it.

- Theme tokens provide background, surface, text, muted, accent, border and text
  scale values. Buttons use 48 logical-pixel minimum height, tab stops and visible
  keyboard focus. Pointer, touch and Enter/Space activation reuse upstream GPUI.
  AccessKit Click invokes the same application callback.
- Button, Label, semantic glyph Icon, Checkbox, Switch, List, ScrollView, Toast,
  status presentation and retained TextInput controllers form the initial catalog.
  TextInput was extracted from the existing shared example, which was adapted
  from the pinned Apache-2.0 upstream input example. It exposes value, label,
  configuration and theme setters. Rust owns draft, selection and composition.
- A shell module provides dialog and bottom-sheet presentation and SafeArea
  padding. Application code owns modality, routes and lifetime. The gallery's
  settings modal renders a separate tree, removes background controls, focuses
  Close, consumes Tab, and restores the retained trigger on Escape/dismiss/back.
- Compact/wide layout switches at 720 logical pixels. Entity-owned controllers
  remain stable across render branches. The gallery includes appearance, large
  text, a settings form, a simple list and loading/empty/error/offline/ready states.
- `Navigation<T>` retains a root and route stack. `Resource<T>` uses request epochs
  to reject stale, duplicate and cancelled completions while preserving saved data
  after a failed refresh. Failures reuse `gpui_system::SystemError`.

## GPUI Kit audit

The inspected GPUI Kit snapshot is
`7cd921d493f32ffefc8ddade4c84d781b2a58c23`. Its workspace pins
`gpui-pre`, platform and macros to `=0.3.12`, while this distribution uses copied
official `gpui` 0.2.2 at `f8cfc7ed9e57503def9479138894f3139f5400cc`.
Directly adding Kit would introduce a second GPUI package identity. Its mobile
platform dependency is gated, and the component layer also depends on its own
base/assets/macros and optional editor/speech stacks. Integration requires a
coordinated port and a real build matrix. This slice reuses official GPUI's
focus, click, semantics and input APIs. A patched Kit build remains a separate
compatibility gate.

The useful reference is Kit's separation of behavior, styling, assets and example
stories. Compare each adopted behavior against the pinned official APIs before
porting. Preserve licenses for any future copied implementation.

- [Kit dependency identity](https://github.com/longbridge/gpui-kit/blob/7cd921d493f32ffefc8ddade4c84d781b2a58c23/Cargo.toml)
- [Kit mobile target gates](https://github.com/longbridge/gpui-kit/blob/7cd921d493f32ffefc8ddade4c84d781b2a58c23/crates/kit/Cargo.toml)
- [Component dependencies](https://github.com/longbridge/gpui-kit/blob/7cd921d493f32ffefc8ddade4c84d781b2a58c23/crates/component/Cargo.toml)

## Validation on November 11, 2026

Use `mise run check:components` for the two navigation/resource model tests and
strict component/shared-page Clippy. Use `mise run build:desktop` for the Windows
MSVC application. The API 35 Android instrumentation suite exercises semantic
text editing, gallery navigation, keyboard activation, modal isolation and focus
restoration through the real host and GPUI runtime.

Windows UI inspection verified a generated application renders, increments by
pointer click, accepts `Desktop draft 你好`, and increments once per Enter after
the duplicate custom key handler was removed. The shared desktop platform page
also rendered with both labeled fields and the gallery entry. Further desktop
gallery interaction was interrupted by concurrent user input and remains an
explicit runtime gate. Android template rendering was inspected from an emulator
screenshot. These checks cover the development environment, with hardware
qualification deferred.

## Remaining acceptance

M2 stays open in #3. The responsive state/data additions and native provider
integration remain substantial work.

- ReactiveScope/Signal/Memo/Binding, Store, QueryClient, Mutation, batched
  invalidation measurements, cache partitioning and logout cleanup.
- Production app bar/bottom navigation and bottom-sheet controllers, richer icon
  assets, list virtualization, reduced-motion preferences and comprehensive
  adaptive state/focus/scroll retention tests.
- A coordinated GPUI Kit port/build audit, complete Windows gallery keyboard and
  screen-reader acceptance, and TalkBack exploration/text-selection/live-region
  acceptance on Android.
- Provider-backed lifecycle/restoration, settings, permissions, files, URL,
  sharing, notifications and the integrated cases from #18–#28. Shared contracts
  and private storage from #19/#22 have been merged. Their providers and remaining
  lifecycle/permissions modules retain their issue ownership.

Integration also includes #20 permissions and #24 activation routing from
PRs #31/#32. Their application/provider acceptance remains in the linked system
issues and gallery integration gates.

The model prototypes establish ownership and stale-result behavior. Their API
and performance need further testing before a stable application-model contract.

## State and data prototype continuation

`reactive` supplies Entity-backed Signal/Memo/Binding/Store and ReactiveScope.
The owning view retains its scope. Equality filters selector notifications,
multiple changes batch one scoped notification, and clear/drop releases subscriptions.
The shared gallery commits its retained TextInput draft explicitly and displays
separate scoped-notification and render-call counters. These counters do not
measure layout, paint or input latency.

`query` supplies a local application-owned QueryClient. Requests are cancelled on
drop/replacement/account change. Stale, duplicate, foreign-client and cancelled
completions are rejected. Refresh errors retain the last successful value.
Mutation reconciliation uses client identity, account epoch and cache revision.
Conflicts require application-driven refetch. The gallery uses controlled fake
completions to exercise loading/data/logout behavior. Transport, automatic refetch,
production ScreenModel service overrides and realtime/master-detail remain open.

The Android host now installs the shared lifecycle service and exposes transient
saved-state hooks. The gallery serializes a versioned route/preferences/counter,
committed value and field drafts with directed selections/focus. Composition from
an old process is finished. See ADR-012/013 and the Android validation continuation.

Five model tests and two real GPUI context tests cover navigation/resource/query,
request cancellation, refresh error retention, duplicate/stale/foreign-client
rejection, account cleanup, mutation conflicts, selector filtering, batching and
subscription disposal. Native-provider and gallery instrumentation add end-to-end
checks for text selection, draft/commit isolation and retained text scaling.
The earlier remaining checklist is historical. Implemented prototypes above
supersede its state/query entries while wider acceptance remains open in #3.

## Native permission recovery continuation

The Android gallery includes notification and camera permission controls backed by
the installed native service. Requests are explicit, statuses are semantic live
regions and settings receipts describe handoff. A panel-owned task retains the
bounded permission stream and is cancelled with its Entity. Foreground reconciliation
updates displayed state after settings. Request callbacks update through weak
Entity handles. Desktop compilation retains its existing shared gallery path.

Notification denial/retry/permanent denial and settings-to-resume authorization,
camera grant and already-granted no-prompt paths are covered by native instrumentation.
Screen-reader speech and physical-device evidence remain deferred acceptance gates.

## Production shell, virtualization and cross-service continuation

This continuation implements the production shell/list/model slice of #3 and the
end-to-end gallery workflows for #18, #23, #25, #26 and #28. The earlier audit
and remaining-acceptance notes above are retained; this section supersedes only
the implementation gaps it explicitly names.

### Shell primitives and motion/accessibility tokens

`gpui-box-components::shell` now provides:

- `app_bar`: an accessible `Role::Header` with a 56px minimum height, optional
  leading slot, scaled title and trailing actions. The bar uses the theme's
  surface, foreground and a visible outline.
- `NavItem` + `bottom_navigation`: an accessible `Role::Navigation` tab bar
  labelled `Primary navigation`, with a caller-supplied container label (the
  gallery uses `Gallery sections`). Every item is a focusable, 48px-minimum
  `Role::Tab` with `aria_selected`, a visible focus ring, an explicit label,
  distinct selected/unselected styling, pointer activation and the matching
  AccessKit Click action. The gallery uses the four unique labels
  `Section: Overview`, `Section: Adaptive shell`, `Section: System workflows`
  and `Section: Local unlock`.
- `BottomSheetController`: retained open state plus separate trigger/sheet
  `FocusHandle`s. `open`/`present` dismiss the virtual keyboard and focus the
  sheet; `dismiss` clears the flag and restores the trigger. `motion_duration_ms`
  reads the theme's reduced-motion policy.
- `keyboard_avoidance` / `keyboard_bottom_inset`: apply the window IME bottom
  inset as padding without changing controller state.
- `master_detail`: one retained tree that switches at 720 logical pixels between
  a compact detail-or-master pane and a wide master plus detail (or empty-state)
  layout.

`Theme` keeps every previous public field and `Theme::DARK`/`Theme::LIGHT` remain
`pub const`. It now derives `Debug` and adds `reduced_motion` and `high_contrast`
flags, `HIGH_CONTRAST_DARK`/`HIGH_CONTRAST_LIGHT` palettes, const builders
`with_text_scale`, `with_reduced_motion`, `with_high_contrast`, and
`transition_ms(standard_ms)` returning zero when reduced motion is enabled.
High-contrast dialogs, app bars, navigation and buttons gain visible borders;
the standard, non-high-contrast button and `TextInput` dimensions are unchanged.

### Deterministic virtualization

`VirtualListSpec` and `VirtualWindow` centralize fixed-height windowing. The
computation clamps invalid/negative/NaN row heights and viewports, an empty
list, an oversized scroll offset and overscan; both spacers conserve the full
scroll extent exactly. `scroll_to_index` and `set_item_count` re-clamp retained
scroll state, so filter-driven shrinking cannot leave an out-of-range offset.
`virtual_list` renders a `Role::List` containing only the visible slice plus the
top/bottom spacers. A 10,000-item test renders only the visible window, while the
gallery filters 500 catalog rows, scrolls and jumps, and keeps the filter draft,
selected item and clamped scroll position across compact/wide transitions.

### Screen model service overrides

`Navigation<T>` exposes its root, route slice and `reset_to_root`. `ScreenModel<R>`
combines that navigation stack with an owned `gpui_system::SystemServices`,
providing `new`, `with_services`, `with_service`, `insert_service`,
`replace_service`, typed `service::<S>()`, service accessors and mutable
navigation access. Registrations are screen-scoped, so tests install fakes
without process globals.

### Gallery workflows

`examples/hello/src/workflows.rs` owns a per-instance scratch root (PID, atomic
sequence and timestamp; removed on drop), private `AppData`, a background
operation context and explicitly registered fake providers. The default gallery
section remains `Overview`; the section bar switches to the new surfaces.

- Workflow 1 opens an authorized document through `FileService`, reads and edits
  the draft with `纪要 · 已审阅 ✅`, saves atomically, stages an
  `ExportResourceLease`, and hands a `ShareResource` to the share service while
  retaining the staged lease. A toggle makes native file sharing report
  `Unsupported`; the workbench then keeps the lease and only copies through an
  explicit, caller-initiated `Clipboard::user_initiated()` action. The fallback
  copies the document snapshot captured at staging time, so later uncommitted
  draft edits never leak into the clipboard, and a fallback requested before
  staging is rejected with an updated status.
- Workflow 2 completes an in-process export, verifies notification permission,
  posts a notification bound to a document activation target, replaces it, and
  routes both a warm callback and a simulated cold relaunch. Replacement before
  the first post is rejected with an updated status. The cold path re-opens the
  durable activation store and inbox over the same `AppData` roots and clears
  process-local activation memory, so duplicate suppression is proven by the
  durable `ActivationInbox` tombstone rather than the consumer-side handled set.
- Workflow 3 probes local-auth readiness, exercises a user-cancelled prompt,
  `BiometricOnly` versus `BiometricOrDeviceCredential` fallback, grants an
  `AccountUnlockManager` unlock and releases an account-scoped `SecretBytes`
  through the capability-gated operation. Release requires an unlocked session
  before the local-auth service applies its own foreground/session-lock gating;
  only the protected key identity and byte length are displayed. Background and
  session-lock transitions relock the account and are asserted to block release.

### Validation and remaining gates

Component tests cover theme builders/motion (including reversible high
contrast), virtual-window clamping and spacer conservation for 10,000 rows,
rendered visible-slice size, `ScreenModel` service registration/replacement/
missing lookup, navigation, app bar/navigation/master-detail rendering, retained
`TextInput` state across compact/wide switches, and bottom-sheet trigger/sheet
focus restoration. The hello tests cover all three cross-service workflows,
including pre-stage fallback rejection and staged-snapshot copying, pre-post
replacement rejection, warm and simulated-cold activation with durable-tombstone
duplicate suppression after process-memory reset, cancelled verification,
biometric/device-credential unlock, strict locked-session release gating, secret
redaction and both relock paths. Gallery tests verify the default `Overview`
section, labelled bottom-navigation switching, focusable catalog rows with
AccessKit activation, 500-item filter/scroll retention, sheet isolation, Tab
cycling and Escape dismissal with trigger-focus restoration.

Remaining acceptance is unchanged where it depends on unavailable externals:
physical iOS/Android hardware qualification, TalkBack/VoiceOver speech and
exploration, a coordinated GPUI Kit port/build matrix, and full Windows
screen-reader acceptance. The fake providers are deterministic test doubles and
are not evidence of native provider behavior. See the issue-linked system
documents for those gates.

## Android adaptive-shell integration qualification

The #81 shared components and gallery run in the API 35 Android host. Three
instrumentation cases cover the 500-item catalog's bounded AccessKit row slice,
jump-to-end, exact filtering, row selection/detail, high-contrast/reduced-motion
controls, bottom-sheet background isolation, Enter/Space/Escape dismissal and
trigger-focus restoration. Section navigation retains the filter draft and
committed query. The actual Android AccessibilityNodeInfo reports selected tabs
before and after navigation through UiAutomation.

AccessKit selected state is now transported in the Android snapshot, applied to
AccessibilityNodeInfo.isSelected and emits TYPE_VIEW_SELECTED on a newly selected
node. This supplies the state already declared by the shared tabs/list rows.
The provider retains its separate text-selection transport. TalkBack speech and
exploration require their own acceptance.

A real portrait-to-landscape-to-portrait Activity/surface recreation retains the
catalog filter draft, committed one-item result and selected item. Wide layout
shows the retained selected detail, while compact layout returns to the list.
The test awaits a newer eligible surface and actual viewport orientation. Test
cleanup finds the resumed Activity after recreation and releases UiAutomation's
accessibility connection without destroying it.

The focused suite passes 3/3 on Android 15 / API 35 x86_64, density 2.625,
4096-byte pages and host Vulkan. Both Android ABIs build and pass strict Clippy.
The preceding integrated Windows qualification passes 16 component tests and
six gallery/workflow tests. Native file/share/notification/auth workflows retain
their operation-specific providers. The new cases exercise the shared shell
and Android semantics, while provider test doubles retain their own evidence.
Physical-device testing remains deferred.

The complete emulator suite passes 20/20 after app-data reset, including the
existing accessibility, permission, lifecycle, editor, responsive-model and touch
tests. Integrated `mise run check` passes 180 host tests plus six compile-fail
doctests and two nested child-process reports. Selected-node runtime evidence
comes from Android UiAutomation. TalkBack speech remains pending.
