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
