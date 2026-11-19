# GPUI host for the retained owner runtime

Status: experimental desktop integration, 2026-11-19. The complete RFC remains
active. Uses the existing [owner input transaction](RETAINED_INPUT_PROCESSING.md),
LayoutStyle/Flex, scoped callbacks and ordinary interaction-state style bindings.
No second hit-test, scrolling or layout solver is introduced.

Follow-up: `--animate` connects the [render-owned timeline](RETAINED_ANIMATION_RUNTIME.md)
to the ordinary count button. Each count change submits one spring translation
target; prepaint samples current time and installs the same complete GPU/CPU frame
with projected input geometry. Repeated clicks redirect current presentation.
This host remains GPUI callback-driven; its UI-stall presentation is not independent.

## Public normalization and host responsibilities

`input::gpui_host::GpuiInputAdapter` is available with retained-text on native
targets. It maps window logical pixels into a retained viewport, primary mouse
down/up/move/leave, wheel line/pixel units, Tab/Shift+Tab, Enter/Space and Escape.
Wheel sign is normalized to positive increases-offset. Primary releases outside
the viewport are forwarded while router capture is active. Modified platform
shortcuts stay with GPUI; an activation release clears a previously accepted hold
even when modifiers changed while held. Its constructor checks finite nonnegative
bounds and a positive finite wheel-line height.

The adapter does not install callbacks, own native capture, or claim presentation.
Hosts must handle focus, window deactivation, callback lifetime, routing order and
image installation. The example attaches key listeners to the actual focusable
GPUI div; canvas-local key listeners would miss that focus dispatch path.
Window mouse listeners preserve release outside the retained area. GPUI/Windows
still own OS capture, and closing/blur sends owner WindowFocusLost cleanup.

## Complete images and matching input geometry

`--retained-owner` opens a separate example rather than modifying the old gallery.
The content is a persistent Flex column, two focusable buttons and a growing
scroll viewport with real bundled Noto text. Hover, pressed and focus-visible
appearance use ordinary StylePlan/computed/bindings. Activate changes count through
the same scoped handler and source transaction on both backends.

The host retains the exact LocalFrame and UiInputScene associated with each image.
It calls adopt_input_scene(ImageInstalled) after adding that image to the native
GPUI scene. This is installation evidence, not compositor/display acknowledgement.
Adoption feedback schedules a subsequent picture; there is no synchronous
adopt-until-stable loop. If adoption fails, input does not continue on mismatched
geometry; a later paint retries the actual installed scene.

During resize the old image stays at its original logical size under the new
viewport clip until a complete replacement is ready. Adapter bounds use the
intersection of installed-image extent and current viewport. Old pixels are not
stretched while their hit coordinates remain unscaled. Resize/DPI requests include
physical size and scale as well as full scene identity, including policy revision.

GPU uses the common WgpuFrameRenderer, capacity observation and completion wakeup.
Windows uses immutable native surface import when available. Device replacement
rebinds completion notification and forces a complete resync; this host recovery
branch has not been fault-injected in this stage. Other platforms or unavailable
native import use the existing synchronous readback bridge, which can stall the
owner. Independent presentation is still open. Initial GPU construction failure
selects CPU fallback and reports the error in the caption.

CPU resolves the owned LocalFrame, rasterizes, converts channel order and builds
the immutable RenderImage on the GPUI background executor. The UI task only accepts
the complete image and its exact input scene. There is at most one active job and
one replaceable pending request, plus one completed image awaiting installation.
Latest pictures can coalesce; input/business transactions remain ordered. Exact
software effects and full text/clipping are retained. A running job is not
preempted, and this is not a cadence guarantee or independent CPU presentation.
Completed CPU images still upload through the GPUI sprite atlas.

## Run and verify

From the research worktree:

```powershell
cargo +1.98.1 run -p gpui-box-example-desktop --features retained-wgpu --locked --offline -- --retained-owner
cargo +1.98.1 run -p gpui-box-example-desktop --features retained-wgpu --locked --offline -- --retained-owner --cpu
```

CPU host also compiles without retained-wgpu. The native diagnostic feature adds
app-internal GPUI event dispatch and explicit framebuffer comparisons:

```powershell
cargo +1.98.1 run -p gpui-box-example-desktop --features native-check --locked --offline -- --retained-owner-check target/owner-gpu
cargo +1.98.1 run -p gpui-box-example-desktop --features native-check --locked --offline -- --retained-owner-check target/owner-cpu --cpu
```

These commands save stage images and result.txt and exit. They explicitly draw and
read back native frames for correctness, and use bounded diagnostic waits. They
do not inject OS input, measure native event delivery, displayed cadence, or
input-to-display latency. A debug software raster may take substantially longer
than a GPU frame; diagnostic waiting is not a performance acceptance threshold.

See [recorded results](performance/2026-11-19-owner-host/README.md).
“99%” expresses common fluent/intuitive behavior; no percentage or p99 gate.

## Open boundaries

This stage does not complete source unification, semantic controls/accessibility,
text editing/IME, multipointer input, task scopes/KeepAlive, hooks/macros, theme
inheritance, resource budgets or independent native presentation. Actual OS input
automation and sustained visible cadence remain separate validation work. Layout
and callbacks still run on the UI owner; CPU raster offloading alone cannot keep
final presentation advancing when that owner is blocked.
