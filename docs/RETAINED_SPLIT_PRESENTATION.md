# Split UI owner and lightweight native presentation host

Status: experimental desktop host, 2026-11-19. Complete RFC remains active.
See [decision](ADR_RETAINED_SPLIT_PRESENTATION.md).

`--retained-split` creates LogicalUi on a dedicated OS thread. That thread owns
its Rc sources, scoped event callbacks, persistent Flex tree and complete scene
publication. The GPUI application thread retains the native window/swapchain and
a lightweight view that installs images, routes input and samples the existing
AnimationRuntime. No LogicalUi, Signal or Window is sent across threads.

```powershell
cargo +1.98.1 run -p gpui-box-example-desktop --features retained-wgpu --locked --offline -- --retained-split
cargo +1.98.1 run -p gpui-box-example-desktop --locked --offline -- --retained-split --cpu
```

Click the upper rectangle (or focus it and press Enter/Space) to redirect a spring
translation. The diagnostic control pauses only the UI owner for500ms. Already
accepted motion continues while new application callbacks/source updates wait.
Tab and wheel remain ordinary owner defaults; they are not independent render-domain
scrolling. Arbitrary hover/pressed callbacks and styles also wait for the owner.
The fixture's text describes the diagnostic ownership split; it is not a shipped
product screen.

## Ownership and ordered publication

The existing `command_channel` carries complete normalized/routed input effects
and viewport/resource requests, FIFO, with capacity64 accepted unresolved commands.
Each response contains the complete UiInputScene, its matching logical viewport,
raster scale, source counter and finite animation controls. The host retains at
most64 unresolved-or-not-yet-adopted requests, including completed response tasks.
Responses are applied in their original CommandId sequence even if foreground
tasks wake in a different order. Controls and input are never latest-only visuals.
Capacity exhaustion explicitly rejects input and leaves prior router state;
there is no claim of unlimited lossless OS ingress. Closing drops the unique
sender, lets already accepted FIFO work finish, and detaches the worker handle
without blocking the platform thread on a join. Unexpected worker loss completes
queued requests with existing executor failure results.

UiInputRouter stays with the actually installed image. Primary down/up, wheel,
Tab/focus and capture cleanup produce owned UiInputEffects. The remote owner calls
the existing process_input_effects, retaining current generation/path/handler and
scroll validation. Its result does not implicitly adopt a newer visual scene.
Image installation adopts the exact projected input scene, and any resulting
enter/leave/feedback is sent back as another ordered owner request.

`AnimationRuntime::activate(source, batch, now)` atomically adopts a complete source
and its causally associated controls. Failed controls leave the old source/timeline,
sequence/IDs/notices intact. `UiInputScene::frame()` exposes the immutable exact
scene so the host never resolves an owner tree by reaching across threads.
The host uses its existing animation stream; ordinary new UI source revisions do
not reset a running or held property. No new renderer or layout truth is added.

## GPU, CPU and environment changes

GPU uses the ordinary WgpuFrameRenderer and native Windows shared surface import.
One GPUI renderer owns final Present, with no second swapchain. Completion wakes,
in-flight capacity and device replacement recovery remain the existing contracts.
Other platforms/unavailable import use the existing synchronous readback bridge;
that path can stall the platform host and is not independently qualified here.

CPU does SoftwareJob construction/raster/image conversion on the background
executor, retains one active complete job, and samples a newer state after the
completed image is installed. It does not continually cancel work on new ticks.
The image and matching sampled input are installed together. Installation requests
the next platform animation frame; a notify while painting alone is insufficient
to guarantee another draw. CPU may run at lower cadence and does not claim120Hz.
Exact text/layout/clip/group-opacity remain; box shadows still explicitly reject
under CPU Exact and are absent from this fixture.
The current debug CPU fixture does not meet interactive animation cadence: a
900x486 raster takes roughly half a second. Moving work off the platform thread
isolates ownership but does not reduce this raster cost. Short100ms/500ms pauses
installed no new complete CPU samples; a separate2s control verifies eventual
image progress. This is not CPU fluidity acceptance.

Viewport/DPI requests carry immutable prepared text and explicit scale. The owner
checks resource scale before layout adoption; a mismatched request leaves its old
source/environment. While a newer viewport waits, the host draws the old complete
image at its original logical size under the new clip, with matching old hit extent.
Sampling uses the accepted source scale, not a newly observed host scale applied
to old glyph resources. Font preparation is still synchronous on the platform
thread for this small fixture. Budgeted fonts/images and environment-wide resource
preparation are not solved by moving LogicalUi.

## Evidence and limitations

The native diagnostic observes normal platform callbacks during live100ms/500ms
owner pauses, plus a2s CPU control. GPU requires distinct installed sample
revisions/placements and new queued NativePresentationAttempt IDs in both short
intervals; CPU records those intervals without imposing a progress minimum and
requires progress only in its separate2s interval. Owner_turn stays fixed. It does not
call window.draw or present_if_needed inside the pause. Explicit readback outside
the interval compares complete native pixels to the ordinary CPU reference.

```powershell
cargo +1.98.1 build -p gpui-box-example-desktop --features native-check --locked --offline
target/debug/gpui-box-example-desktop.exe --retained-split-check target/split-gpu
target/debug/gpui-box-example-desktop.exe --retained-split-check target/split-cpu --cpu
```

Run as an ordinary visible window. A hidden process/window does not establish
normal presentation; the diagnostic rejects hidden state. Native Present S_OK
is OS submission acceptance, not compositor completion or photon timing. This
host establishes a possible ownership split relative to component/Flex stalls;
blocking the GPUI platform thread, saturated GPU/CPU, synchronous resource preparation,
driver stalls and the readback bridge can still interrupt it. Native CPU means
software-rendered retained content uploaded through the normal GPUI image path;
it is not a GPU-free operating-system presentation backend.

This is an explicit integration example, not a universal application runtime or
automatic migration of the old owner/gallery. General factory/lifecycle transport,
input priority/backpressure recovery, portable resource services, budgeted pending
scenes/retirement, direct scroll/drag and five-platform presentation qualification
remain open. See [results](performance/2026-11-19-split-presentation/README.md).
No new FPS/tail/success-rate guarantee; “99%” remains figurative.
