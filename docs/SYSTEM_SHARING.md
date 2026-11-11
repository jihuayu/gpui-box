# Explicit sharing and authorized export lifetime

`gpui-sharing` implements the sharing slice of [#25](https://github.com/jihuayu/gpui-box/issues/25).
Applications opt in with the facade's `sharing` feature. Common capabilities,
errors, UI ownership and cancellation come from `gpui-system`; URL policy comes
from `gpui-activation`; authorized resources and staged export pins come from
`gpui-files` and `gpui-app-data`. There is no second picker or resource model.

## Calling from a user action

`Sharing` is UI-local and bound to a `UiContext`. Call `user_initiated()` directly
from the foreground user's share action and consume its single-use
`ShareInteraction` with `share`. A token from another service is rejected even
when the caller reuses a surface number. Cloning a service preserves its identity.
Dropping or advancing the host `UiOwner` invalidates its old interactions. The
host must recreate the service when its native attachment changes.

Requests contain a bounded title and text, HTTP(S) URL, or prepared authorized
resource items. Text is unchanged, including CJK, emoji and newlines. A URL is
revalidated at execution even when its `AllowedUrl` originated in deserialization.
Debug output does not expose titles, text, URLs, or resource paths. Invalid anchor
geometry is rejected before native work; platform adapters that require an anchor
must additionally map it to their native surface.

Capability probes do not present UI. Text, URL, files, multiple items, target
results and copy are independently described. A provider must check fresh native
conditions before invocation. A `DataPackage`'s alternate representations do not
mean a provider supports multiple independent share items.

## Route and receipt semantics

The caller selects `ShareRoute::Native` or `ShareRoute::CopyText` before execution.
A native failure, denial, unavailable service or cancellation never silently
copies content. Copy accepts exactly one text or URL item; it does not flatten
multiple items or expose a file path. Install a `CopyTextProvider` explicitly to
permit that route. `WindowsSharing::attach_with_copy` explicitly installs the
production clipboard bridge on the same HWND/UI attachment. It forwards the
sharing cancellation token and deadline into the single-use clipboard interaction.
A copy provider must report native acceptance rather than wrap
a void API that swallows failures.

`SubmittedOutcomeUnobservable` means the OS accepted the invocation of sharing
UI. The program did not observe visible presentation, target selection, target
acceptance or delivery. `FallbackPerformed { CopyText }` means the selected copy
route published text to the OS clipboard. It never means that another application
pasted the content. User/target completion is not synthesized.

The synchronous method checks cancellation/deadline before effects. A cancellation
that arrives after an accepted native handoff does not erase its actual receipt.
A native callback's later state and lifetime remain provider-owned independently
of the Rust return value. Cancelled waiting cannot roll back a share already
submitted to the OS.

## Authorized files

`ShareResource::prepare` runs on a background executor without UI attachment.
It streams an existing opaque `FileResource` into an `ExportResourceLease`,
retaining the original authorization. Its immutable `Arc` lease stays pinned
while any request or consumer clone exists. No caller-supplied path is accepted.

A provider supporting files must clone the prepared `ShareResource` and retain it
until the actual consumer completes or a documented controlled retention deadline
expires. Panel dismissal, UI disposal and the submission receipt are not consumer
completion. `export_lease()` is a trusted provider handoff boundary; its staged
path must not be serialized or logged. This release does not claim native file
sharing: Windows `Files` and `MultipleItems` report `Unsupported`. The async
StorageFile/data-provider bridge, Android content URI grants and Apple item
providers require their own native lifetime qualification.

Tests exercise a delayed file consumer across request drop, UI disposal and
`cleanup_exports(Duration::ZERO)`. Cleanup preserves the pin until the last
consumer releases it, and can remove the export afterwards. Preparing a file is
blocking work and is never hidden inside the share-sheet UI method.

## Windows adapter

`windows::WindowsSharing::attach(hwnd, ui)` creates a bound `Sharing`. The host
retains its actual HWND and `UiOwner`; numeric HWND storage never owns a window.
Attach and invocation verify window process, UI thread and a current STA/main-STA
COM apartment. Invocation also requires that exact HWND to be visible and
foreground. The adapter queries COM ownership without initializing/uninitializing
the host apartment. Missing/incompatible apartment returns configuration errors.

Windows 10+ `DataTransferManager` HWND interop submits one text or HTTP(S) link.
The adapter installs its removable `DataRequested` listener before calling
`ShowShareUIForWindow`, without holding Rust borrows across native callbacks.
Only one adapter may attach to the same HWND at a time, so independently created
services cannot register competing DataRequested listeners for one window.
One pending data request occupies the provider lane. Until data is supplied or
its 120-second controlled request expiry is observed, a subsequent share returns
`Busy`. Callback duplication cannot claim the same payload twice. A stale UI
attachment or expired callback is rejected. Successful text/link preparation
copies representations into the OS-owned DataPackage, after which the provider
can retire that listener on the next invocation. Later caller cancellation does
not revoke the already accepted invocation. Provider drop removes its listener;
applications must retain the service while sharing is pending.

`cargo run --locked -p gpui-sharing --features gpui-example --example windows_share`
opens a standalone GPUI example. The example extracts a borrowed Win32 handle from
GPUI, retains a `UiOwner` with the view, and submits unchanged Unicode text only
on click. Its status explicitly describes invocation rather than delivery. This
example is manual and is not run by tests. No host/CLI/gallery ownership files
are changed by this crate.

## Evidence and remaining gates

Evidence collected on Windows x86_64 on 2026-11-11, using `cargo +1.98.1`
(custom host Cargo 1.101; rustc 1.98.1):

| Platform | Implementation/evidence | Remaining qualification |
| --- | --- | --- |
| Windows | Native single text/web-link adapter compiled; lifecycle/error contract tests; GPUI example compiled | Manually observe panel, targets, cancellation, process/attachment recreation, packaged/unpackaged host behavior; native files/multiple items unsupported |
| Android arm64/x86_64 | Shared contract cross-check | ACTION_SEND/SEND_MULTIPLE, Sharesheet, FileProvider URI grants, Activity recreation and device evidence; current native adapter unavailable |
| macOS | Shared contract | NSSharingServicePicker delegate lifetime and anchor mapping; no native provider |
| Linux | Shared contract | Desktop capability discovery and explicit installed copy/mail route; no native share provider |
| iOS | Shared contract | UIActivityViewController, NSItemProvider, iPad anchors, completion and device qualification; no native provider |

Neither compilation nor fake-provider tests establish OS panel visibility or
successful delivery. The shared emulator/app installation is owned by another
platform wave and is not modified by this work. Mail fallback, rich clipboard
formats, drag/drop, native file providers and observable target completion are
future scopes, not inferred capabilities.

Validation commands:

```sh
cargo +1.98.1 test --locked -p gpui-sharing
cargo +1.98.1 clippy --locked -p gpui-sharing --all-targets -- -D warnings
cargo +1.98.1 check --locked -p gpui-sharing --features gpui-example --example windows_share
cargo +1.98.1 check --locked -p gpui-box --no-default-features --features sharing
cargo +1.98.1 check --locked -p gpui-sharing --target aarch64-linux-android
cargo +1.98.1 check --locked -p gpui-sharing --target x86_64-linux-android
```
