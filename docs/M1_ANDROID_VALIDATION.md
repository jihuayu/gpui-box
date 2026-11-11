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
| `mise run check` | Passed 50 host tests after integrating #31/#32, one compile-fail doctest, formatting, strict Clippy and whitespace checks |
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

## Host integration and accessibility continuation

The host installs the merged `gpui-lifecycle` handle in GPUI SystemServices before
application launch. Generation-gated observations cover Activity started/resumed,
window focus, OS appearance, font scale, locale, disabled-animation preference,
keyguard and memory warnings. Input observations use monotonic time. High contrast,
network reachability and measured platform idleness retain unavailable status.
The GPUI window also receives appearance changes through its existing callback.
Started visibility and resumed/focused input state remain separate.

AccessKit geometry is already physical pixels in the pinned engine. The adapter
now composes ancestor transforms and avoids multiplying Android density twice.
TextInput supplies an actual TextRun and directed scalar selection. Android maps
selection actions to UTF-16, rejects malformed/out-of-range input and retains
focused-run identity. Virtual focus, subtree text queries, hover enter/exit and
text/selection/live events are transported through AccessibilityNodeProvider.
Hover chooses the deepest exposed node in reverse sibling paint order. Wider
clipping/transformed fixture coverage and TalkBack speech remain open gates.

Applications register `on_saved_state`, consume `take_saved_state` through launch's
App context, and encode their schema. The example reuses VersionedSnapshot for
route, preferences, counter, committed text, drafts, directed selections and focus.
Old connection composition is finished. Bundle payloads are bounded to 128 KiB.
Activity recreation keeps the live Rust model authoritative. New processes consume
the saved Bundle once. This is transient UI restoration. Durable business state
and activation inboxes use their application services.

The API 35 emulator passed a cold custom-scheme launch, exposing `Activation events
1`, and a real warm ACTION_VIEW delivery in instrumentation. The bounded pending
queue retains IDs across Activity recreation. Application acceptance is explicit.
The host supports 16 pending events and a 256-ID process duplicate window. Durable
routing requires the #24 inbox, and overflow is logged without raw URLs.

Manual process reclamation kept the Android task, sent HOME, called `am kill`,
confirmed the old PID exited, and reopened the launcher task. PID 12771 was replaced
by 12928. Gallery route and draft `ProcessRestoreQ` survived. Activation count
reset to zero as expected for a new runtime. The restored focus reopened the IME,
so the keyboard was dismissed before querying the offscreen field's semantics.
This exercises Android process recreation on the emulator. Force-stop is a separate
operation and is not a saved-task restoration contract.

The connected suite now has 11 tests. New native provider checks travel through
UiAutomation's Android accessibility connection for roles, editing, text selection,
virtual focus and click actions. Other additions cover committed/draft isolation,
large-text retention, query cleanup, stale callbacks and Intent deduplication.
Tests release their accessibility connection and restore the platform page between
cases. The navigation-only back case keeps accessibility disabled after IME hiding,
preventing test-induced focus/IME reactivation.

The remaining M1 gates include TalkBack speech/gesture exploration and live-region
qualification, device-loss recovery, wider pointer cancellation/gesture stress,
combined predictive-back/IME animation, full edge-to-edge insets, and deferred
ARM64 hardware/multi-vendor/real Chinese keyboard candidate UI. #2 remains open.

Validation commands for this continuation include `mise run check`,
`mise run check:components`, `mise run build:desktop`, cargo-ndk x86_64 strict
Clippy, Gradle lint and the full connected suite. Lint reports 0 errors and
8 warnings. Upstream linker messages and Android deprecation warnings remain
visible. A Windows files test now explicitly closes its verified reader before
atomic replacement, coordinated in #23. This restores portability of that test.

Final continuation verification passed the connected suite 11/11 in two
consecutive runs. The expanded `mise run check` after PR #40 integration passes
115 host tests and 3 compile-fail doctests, with one additional child-process
test invocation. Components pass 5 model tests and 2 real GPUI context tests.
Windows desktop build and Android x86_64 strict Clippy pass.

## Native contextual permission continuation

Session `android-permissions-20261111-b731` coordinates #20/#2 and the #3/#4
gallery/tooling integration. The Android host installs the process-owned shared
PermissionService in SystemServices before application launch. Both a purpose
declaration and the matching manifest permission are required. Missing microphone
configuration returns InvalidConfiguration with RepairConfiguration. Unsupported
photos/biometrics/special access remain explicit Unsupported results.

The adapter maps notifications, camera, microphone, foreground location and contacts.
It probes checkSelfPermission on every query and result, handles approximate
location as Limited and enterprise policy restriction as Restricted. Only completed
denied callbacks contribute persistent first/retry/permanent-denial hints. A later
successful grant clears a permanent-denial hint. Auto-reset/reinstall and wider
OS policy cases require further qualification. Authorization must be rechecked
before the actual protected operation and operation-time denial remains possible.

Kotlin posts prompt presentation until Rust borrows unwind, then verifies the
request is still current. Unique process request IDs identify callbacks. The
native lane remains busy while Android owns a dialog even if the application
cancels. Surface replacement advances the host UiOwner and rejects old completions.
Saved pending native state carries a process token and records only started dialogs.
Foreground-focus transitions reconcile authoritative states through the shared
bounded observation stream. Settings results describe handoff only.

New instrumentation exercises actual system permission controls through UiAutomation.
Camera grants resolve the Rust request and a repeat request reports prompted=false.
Notification first denial permits retry, the second denial becomes permanent,
and a repeat avoids a synthetic dialog. The notification settings Activity is
actually opened, PackageManager grants authorization while the app is paused,
and foreground reconciliation updates the semantic status. Unknown request IDs
are rejected. Configuration/unsupported cases use the native Rust diagnostic path.
No notification payload or camera image is created by these tests.

During test development, `pm revoke` correctly killed the instrumented app after
a grant. That path requires an external process runner. A stale emulator Settings
ANR overlay also caused focus failures. Clearing it and waiting for the actual
Settings window before returning resolved the test interference. Keep lint and
connected tests serial on this host. Do not classify an obscuring system window
as a provider authorization result.

The full API 35 x86_64 connected suite passed 14/14 after these fixes. Both Android
ABIs build at platform 35, Windows desktop build passes, and Android strict Clippy
passes. Integrated host checks include 137 host tests plus 6 compile-fail doctests
after #41, with two additional nested child-process test reports. Components pass
5 model tests and 2 actual GPUI context tests. CLI passes 11 tests. Android lint
reports 0 errors and 8 existing warnings. Toolchain/renderer details match the
continuation above. Physical-device testing remains deferred.

Runtime evidence covers notification denial/retry/permanent denial/settings-resume
and camera grants. Microphone, contacts, approximate-location, policy restrictions,
prompt-during-recreation/cancellation stress and revocation/process reconstruction
retain separate runtime gates. Provider capabilities conservatively use Compiled.
The generic five-platform contract manifest retains its original scope. Full M1,
M2 and system-service acceptance remain open in their issues.

## Touch contact lifecycle continuation

GpuiView now accepts touch input only on its main thread while attached and
resumed. A new ACTION_DOWN cancels the previous contact stream, including an
interrupted stream that never received UP/CANCEL. A repeated pointer-down also
cancels that pointer's old native contact before allocating a fresh identity.
The native bridge rejects background Started/Moved/Ended events while permitting
Cancelled events to release the recognizer. Android pointer IDs remain separate
from array indices and process-monotonic GPUI contact IDs.

TouchLifecycleTest drives real Kotlin MotionEvent objects into the host and the
existing upstream recognizer. Three cases exercise 20 cancellation cycles plus
late UP, cancelled pans versus taps, replacement DOWN, repeated pointer IDs,
reordered pointer indices, secondary-pointer release before/after the primary,
pause/late-event rejection and ten successful taps after resuming. Assertions
observe application counter callbacks through their corresponding committed-back
handlers. They preserve the existing example page and component ownership.

Initial test attempts incorrectly expected the unlabelled platform counter in
the accessibility tree. The assertions now use the existing application behavior.
A completed pan can start upstream inertia, whose first subsequent touch stops
the fling. The cancellation case explicitly sends ACTION_CANCEL and verifies a
fresh tap afterward. Inertial-scroll stop behavior retains its upstream policy.
The focused three-test suite passes on Android 15 / API 35 x86_64, 4096-byte pages,
with the same Vulkan emulator configuration as earlier qualification.

Both Android native ABIs build at platform 35, and both pass strict Clippy.
Integrated `mise run check` passes 180 host tests plus six compile-fail doctests
and two nested child-process reports. With collaborator PR #81 integrated,
Windows passes 16 component tests and six shared-gallery/workflow tests.
`mise run check:components` passes strict gallery/component lint. Android lint
reports zero errors and eight existing warnings.
The full API 35 emulator instrumentation suite passes 17/17 after app-data reset,
including all previous 14 cases and the three new touch lifecycle regressions.

This slice adds contact-stream stress. Long-press claims, full scrolling physics,
GPU device loss, combined IME/predictive animation, TalkBack speech, ARM64 hardware
and real Chinese keyboard candidate UI retain their independent acceptance gates.
Physical-device testing remains deferred.
