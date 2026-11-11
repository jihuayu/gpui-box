# M1 Android foundation progress

## Scope on 2026-11-11

M1 remains in progress. This slice implements focus-scoped InputConnection
lifetimes, clipboard context actions, nested batch notifications, committed
system-back routing, memory-warning forwarding, and stale host-request rejection.
Android requires API 35. Physical-device testing is deferred during the current
phase under the project owner's updated scope.

## Runtime changes

- `WindowState` tracks a focus identity and monotonically increasing input epoch.
  The epoch changes when focus changes or the window becomes inactive. GPUI
  reinstallation of the same focused handler during repaint preserves the epoch.
- Input connections carry the surface generation, focus epoch, and host instance
  identity. Queries and writes fail after replacement, close, pause, detach, or
  focus changes. Rust validates both generation and focus eligibility before
  accepting edits. Android's default View handler executes connection callbacks
  on the main Looper, and the adapter checks that thread before accessing Rust.
- Nested batch edits apply changes immediately to the authoritative Rust model.
  Selection notifications are deferred until the outer batch completes. Closing
  an eligible connection finishes its composing range and clears outstanding
  batch depth before pause or detach invalidates the native session.
- Context-menu select-all/copy/cut/paste uses GPUI's current selection and the
  Android plain-text clipboard. The platform clipboard API uses the same JNI
  service functions. Service calls release internal callback borrows before
  application execution. Clipboard JNI exceptions are cleared and logged.
- Host requests snapshot view attachment and surface identities. Non-frame work
  is dropped after either changes. A detached old view cannot clear a newer
  view's active surface identity. Frame wakeups remain process-runtime work.
- An API 35 `OnBackInvokedCallback` dismisses visible input first, then dispatches
  an enabled platform callback or GPUI portable back listener, then finishes the
  Activity if unclaimed. Portable button/committed events use the viewport center
  as their synthetic start position. The example's positive counter installs a
  root back listener which decrements the counter when committed. Clicking the
  counter clears text focus and dismisses input before navigation.
- `onTrimMemory` at UI-hidden/background levels and `onLowMemory` forward to
  GPUI's memory-warning callback. Rendering and edits remain main-thread owned.

## Validation

The environment is the API 35 Android 15 x86_64 emulator documented in
[M0 validation](M0_ANDROID_VALIDATION.md). Both ARM64 and x86_64 native libraries
are built with cargo-ndk platform 35 and packaged in the debug APK.

| Check | Result |
| --- | --- |
| `mise run check` | Passed formatting, 3 host tests, distribution Clippy and whitespace checks |
| `mise run check:desktop` | Passed Windows/shared-page compilation |
| `mise run build:desktop` | Passed Windows MSVC executable build |
| Desktop/shared-page Clippy with `-D warnings` | Passed |
| Android x86_64 Clippy with `-D warnings` | Passed |
| Gradle `:app:lintDebug` | Passed with 0 errors and 7 warnings |
| `mise -E android run test:android` | Passed all 6 connected regression cases on API 35, with both ABI builds |

The connected suite exercises the actual Kotlin adapter, JNI exports, GPUI
focused handlers, Rust text state, clipboard service, and Activity lifecycle.

- Chinese composing replacement/commit, UTF-16 ranges, surrounding text, emoji
  selection/replacement/deletion, and stale surface generation rejection.
- Two nested batches, selected copy/cut/paste, invalid negative delete lengths,
  and unsupported context actions.
- Two fields, stale focus epochs, same-field repaint, replacement connections,
  explicit close, pause/resume, and retained independent field contents.
- Portable back claim/commit behavior and continued editing after trim/low-memory
  callbacks.
- Activity recreation with a new surface generation, retained Rust text, old
  connection rejection, and editing through the replacement Activity.
- Android system back key routing to the GPUI counter listener.

The system-back regression isolates navigation with Android's alternate-IME-focus
window flag and waits for window-manager/Choreographer delivery. Combined IME
show/hide and back-gesture transitions remain a separate acceptance case.

APK output is `platforms/android/app/build/outputs/apk/debug/app-debug.apk`.
Connected XML results are under
`platforms/android/app/build/outputs/androidTest-results/connected/debug`.
Build outputs remain ignored. Upstream Windows linker informational warnings
and the existing deprecated AndroidX ActivityTestRule import warnings remain.
Android Lint reports the pinned target API and three test dependency versions,
backup-rule metadata, missing example icon, and the custom SurfaceView's pending
accessibility click integration. Packaging metadata and AccessKit work own those
remaining warnings.

## Remaining M1 work

The following acceptance items remain open in issue #2.

- Complete text input configuration, editor actions, caret geometry, real Chinese
  keyboard candidate UI, and additional malformed/surrogate-boundary cases.
- Predictive-back progress/cancellation, edge-based claiming, animated insets,
  and keyboard/inset interaction cases.
- Process-death save/restore, lock/unlock, GPU device-loss recovery, and broader
  gesture/cancellation stress tests.
- AccessKit Android integration and TalkBack acceptance.
- A documented panic-containment/shutdown policy across all JNI exports and
  consistent native-entry thread guards.
- Deferred ARM64 physical-device and multi-vendor rendering qualification.

The memory test checks runtime usability after notifications. Cache reclamation
and pressure-driven GPU resource rebuilding require separate measurements.
