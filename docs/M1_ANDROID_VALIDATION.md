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

## Follow-up scope after the earlier foundation slice

The following items were carried forward from the six-case baseline in issue #2.
The additional slice below records subsequent implementation and current gaps.

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

## Additional platform and gallery slice

The following implementation extends the earlier six-test baseline. It retains
the official GPUI snapshot and adds distribution-owned behavior.

- `android_entrypoint!` generates the typed JNI exports from an application launch
  function. The example and generated projects share this macro. Every entry
  checks Android's main Looper before accessing thread-local GPUI state.
  `catch_unwind` converts Rust panics to IllegalStateException and poisons the
  process-owned runtime. Subsequent entries reject execution and require process
  restart. Native aborts remain process failures. The default Activity leaves
  that exception uncaught, so a partially unwound runtime cannot resume normal
  rendering. Recoverable renderer errors still need a separate device-loss path.
- EditorInfo derives autocorrect, capitalization, suggestions and action flags
  from GPUI. Done dismisses input, Next/Previous dispatch Tab/Shift-Tab, and other
  actions dispatch Enter. Hardware navigation and shortcuts enter normal GPUI
  key-down/up handling. CursorAnchorInfo receives physical-pixel caret geometry,
  screen translation, UTF-16 selection and composing text on immediate/monitor
  requests. Advanced cursor update filters return unsupported.
- `OnBackAnimationCallback` forwards start/progress/cancel/commit. Claimed gestures
  retain their original local position. Pause/detach cancel active gestures.
  A gesture unclaimed at its actual edge cannot gain a synthetic center claim
  at commit. Button navigation retains the existing committed-back path.
  Android 15 sends a synthetic `(0, 0)` animation start for `KEYCODE_BACK`.
  The host recognizes that positionless sequence and uses committed-back
  dispatch. Real gestures keep their original edge hit test. The connected
  system-key case caught this distinction and passed in two consecutive full
  eight-case runs after correction.
- WindowInsetsAnimation updates the fitted SurfaceView's system/IME padding.
  GPUI receives zero residual insets because the content viewport is already
  fitted. Full edge-to-edge delivery remains open. SafeArea reads the coordinated
  WindowInsets API without applying host padding a second time.
- The initial Android AccessKit transport retains incremental nodes, prunes
  unreachable subtrees and exposes semantic virtual nodes through an
  AccessibilityNodeProvider. It maps roles, names, values, enabled/toggled/focus
  state, bounds and Click/Focus/SetValue actions. Callbacks execute after internal
  borrows are released. Surface generation and active-state checks reject old
  requests. Recreated Views rebuild their virtual-node snapshot.

The eight connected tests extend the baseline with editor configuration, cursor
geometry and update modes, native main-thread rejection, semantic text editing,
gallery controls, Enter activation, modal background isolation, Escape dismissal
and trigger focus restoration. Existing lifecycle, clipboard, composition,
surface, memory and back cases remain in the same suite. Predictive cancellation,
progress, commit and stale-surface checks are exercised directly through JNI.
Surrogate-interior and extreme UTF-16 selection tests cover scalar-boundary
clamping. InputConnection candidate UI with a real Chinese keyboard remains open.

### Validation for the additional slice

| Check | Result |
| --- | --- |
| `mise run check` | Passed 30 host tests, one compile-fail doctest, formatting, strict Clippy and whitespace checks |
| `mise run check:components` | Passed two model tests and strict component/shared-gallery Clippy |
| `mise run build:desktop` and desktop strict Clippy | Passed on Windows MSVC |
| Android x86_64 strict Clippy | Passed for the backend and example JNI exports |
| Gradle `:app:lintDebug` | Passed with 0 errors and 8 warnings |
| Gradle `:app:connectedDebugAndroidTest` | Passed 8/8 in three consecutive full runs on API 35 after the synthetic back-key fix |

Both Android ABI builds use platform 35. Repeated builds verified Gradle's native
task becomes UP-TO-DATE without source changes and reruns after component/host
changes. Android Lint passes with eight warnings in the development host,
including target/tool/dependency updates, backup/icon metadata and SurfaceView
accessibility click guidance. Upstream Windows linker messages and deprecated
Android node/test APIs remain visible in tool output.

The platform's synthetic back-key sequence was checked against Android 15
`ViewRootImpl.NativePreImeInputStage.doOnBackKeyEvent`.
[Android 15 source](https://github.com/aosp-mirror/platform_frameworks_base/blob/android-15.0.0_r1/core/java/android/view/ViewRootImpl.java)

M1 remains open for TalkBack runtime acceptance, virtual-node hover exploration,
text-selection actions, composed transforms, live announcements and modal
screen-reader traversal. Process-death restore, lock/unlock, GPU recovery,
broader gesture stress, combined IME/predictive-back animation and full edge-to-edge
insets also remain open. #21 owns application-facing restoration/lifecycle
contracts. #2 owns their host integration. Physical-device, multiple GPU vendor
and real Chinese candidate-UI qualification remain deferred.
