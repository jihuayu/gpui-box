# Architecture decisions

Prepared on November 11, 2026. User requirements are accepted. Technical choices
below are proposed defaults pending the Android feasibility milestone.

## ADR-001 — Deliver a coordinated distribution with a small upstream patch surface

### Context

Application developers need a consistent GPUI version, reusable components, mobile
support, and release tooling. GPUI and Kit evolve independently and can expose
incompatible types if multiple GPUI package identities enter the graph.

### Decision

Provide an application-facing `gpui-box` facade over a tested, fixed dependency set.
Anchor platform work to official Zed GPUI, preserving its workspace and coordinated
crate set at one pinned revision. Add `gpui_android` inside that workspace and keep
necessary upstream changes in a documented patch queue. Use optional compatible
Kit layers once the official baseline works. Select the exact package graph
during M0 and make graph inspection a release check. Official source is copied directly under `vendor/gpui`, including its local
build/test dependency closure. The original workspace manifest and file hashes
are retained. The root distribution workspace uses local path dependencies.

### Alternatives considered

- A permanently independent engine fork offers control but increases upstream
  merge and compatibility work.
- A component-only library has a smaller surface but leaves Android and release
  integration to each application.

### Consequences

The distribution owns compatibility testing and migration guidance. Consumers
must use the facade's coordinated GPUI identity. Upstream snapshots are adopted
through validated upgrades.

## ADR-002 — Use an Android native host with Gradle-owned packaging

### Context

Android needs platform lifecycle, native surfaces, text composition, Java-thread
services, resources, signing, and application-store packages.

### Decision

Use a thin Kotlin Activity/SurfaceView host with a typed JNI bridge and direct
InputConnection integration. The system main thread owns GPUI state, layout,
input, and initial GPU submissions. Android drives foreground tasks and frames.
Use cargo-ndk for Rust ABI builds and the Gradle Wrapper for Android packaging.
Evaluate the existing GPUI WGPU renderer with Vulkan first. Confirm the host, renderer, text
stack, accessibility bridge, and full tool tuple in M0.

### Alternatives considered

- GameActivity provides useful text/lifecycle glue, while android-activity's
  native-thread loop would change the selected main-thread ownership model.
- NativeActivity simplifies an initial host but requires additional text-input work.
- Makepad's integrated tooling is a workflow reference whose renderer and UI model
  would introduce a second framework if embedded directly.

### Consequences

gpui-box maintains the Android host and dispatcher integration directly. Background
work returns to the system main thread through explicit dispatch. Native reentry,
GPUI borrows, and main-thread blocking require clear invariants. M0 can revise the
host or rendering choice if measurements fail the required acceptance cases.

## ADR-003 — Make mise the developer tool and task entry point

### Context

Rust and Android builds depend on several tool versions. Contributors need a
consistent setup across Windows and CI.

### Decision

Pin shared Rust tools and tasks in `mise.toml`. Pin Temurin Java and cargo-ndk
in the opt-in `mise.android.toml` environment for Android development.
Let Android SDK tooling manage SDK packages and license acceptance, with explicit
SDK/NDK pins added by M0. Let the Gradle Wrapper own Gradle. Avoid duplicate Rust
version declarations unless a documented consumer requires one.

### Consequences

The checked-in configuration is the tool-version source of truth. Contributors
still provision host compilers and Android SDK prerequisites. CI will run the
same mise tasks and Gradle Wrapper as local development.

## ADR-004 — Use channel-specific update providers

### Context

Desktop installers, Play-managed applications, and directly distributed Android
packages use different installation and trust mechanisms.

### Decision

Expose a shared update state machine with explicit provider capabilities. Android
uses Play delivery or verified APK handoff to the system installer. Desktop update
installation is implemented in the parallel desktop distribution track. Signed
metadata and application identity checks are part of the direct-download protocol.

### Consequences

The API represents user-action requirements and unsupported operations. Tests
must exercise actual distribution channels. Android recovery respects versionCode
and signer rules. Platform-level rollback promises require their own evidence.

## ADR-005 — Preserve reusable interaction behavior and add adaptive presentation

### Context

Kit contains substantial existing component work. Android introduces touch,
keyboard occlusion, navigation, font scaling, and accessibility requirements.

### Decision

Audit and reuse compatible behavior/components, then add mobile presentation and
application patterns where needed. Validate a focused catalog in a shared gallery.
Keep application services and heavyweight optional features separable.

### Consequences

Shared views remain practical while platform differences stay visible. Component
availability is documented against the tested platform matrix. Desktop-only
dependencies require feature or target isolation.

## ADR-006 — License original contributions under Apache-2.0

### Context

The user selected Apache License, Version 2.0 for gpui-box.

### Decision

Apply Apache-2.0 to original project contributions. Declare `license = "Apache-2.0"`
in future workspace package metadata. Preserve third-party licenses, copyright
headers, and applicable NOTICE obligations when incorporating upstream work.

### Consequences

The repository includes the complete Apache-2.0 text. Release packaging will
produce a third-party inventory and notices based on actual included dependencies
and assets. Copyright ownership follows the relevant contributors.

## ADR-007 — Separate runtime, view attachment, surface, and GPU resources

### Context

Android activity recreation, surface replacement, backgrounding, and GPU device
loss have different lifetimes. Delayed work can outlive its native objects.

### Decision

Track `GpuiRuntime`, `ViewAttachment`, `SurfaceSession`, and `RendererResources`
independently. Tag asynchronous work with generation-aware handles and cancel
obsolete requests. Before a surface-destruction callback returns, finish all host
access to that surface. Persist application-owned serializable state for process
recreation. Preserve GPUI's existing foreground/background executor distinction.

### Consequences

Lifecycle transitions become explicit test targets. Surface recreation can retain
application state and reusable GPU resources. Main-thread ownership simplifies
text and native UI integration. A later rendering-thread change requires evidence
and ownership-safe frame data.

## ADR-008 — Reuse mobile protocols and stage native-view integration

### Context

Official GPUI already includes raw touch, gesture processing, UTF-16-based text
interfaces, window insets, AccessKit semantics, and an iOS backend.

### Decision

Adapt Android to those existing protocols. Keep Rust authoritative for editable
text. Reuse the existing iOS/Apple path during the later iOS validation track.
After the self-rendered application is verified, introduce a restricted native
view host with rectangular bounds and simple overlay ordering. Design external
textures independently.

### Consequences

Mobile work extends existing GPUI behavior and requires focused interoperability
tests. Initial Android delivery stays bounded. Native composition and iOS
certification have their own acceptance gates.

## ADR-009 — Stage Android system composition behind measured interop gates

### Context

Native views and GPU-rendered content need matched geometry, buffer ownership,
and traversal timing. Flutter HCPP demonstrates an experimental system-composition
approach whose APIs and renderer assumptions require gpui-box-specific validation.

### Decision

Follow the base Android release with C0–C5 in ANDROID_COMPOSITION_PLAN.md. Prove
AHB/WGPU target and external-fence interoperability first. Preserve a basic
native-overlay tier, then add an opt-in system tier using sealed frames,
generation-aware ownership, bounded queues, ViewRoot synchronization, and exact
release tracking. Treat API 34+ as the initial system-tier candidate policy while
checking the actual minimum for each called API.

### Consequences

The base Android delivery has an independent release gate. System composition
needs device capability negotiation, failure recovery, and resource stress tests.
Shared composition types are extracted after Android and iOS demonstrate matching
needs. Renderer and host changes remain narrowly scoped and separately reviewed.

## ADR-010 — Manage desktop, mobile, and components in one distribution repository

### Context

The user clarified that gpui-box manages desktop and mobile together, including
future reusable component libraries. The user requires upstream source to be
copied directly into this repository.

### Decision

Keep imported engine/backend code under `vendor/gpui`, distribution APIs/services
and CLI under `crates`, reusable UI under `components`, runnable consumers under
`examples`, and native hosts/packaging under `platforms`. Preserve desktop platform
source and expose desktop application startup through the common facade. Android
is the first new backend. Desktop packaging can progress independently after its
shared tooling prerequisites exist.

### Consequences

Desktop development uses the base Rust mise environment. Android tools are opt-in.
Each platform has separate build/device validation while sharing source, APIs,
components, and release management. Imported source is versioned directly with
provenance and retained license material.
