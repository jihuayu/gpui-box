# Reference implementation research

Inspected on November 11, 2026. Commit links identify the source snapshots checked
during planning. They establish research provenance. A compatible implementation
dependency graph will be selected and tested in M0.

## GPUI and the component ecosystem

### Zed GPUI

The platform constructor provides a concrete map of backend selection. Android is
absent from that constructor at the inspected revision. Platform traits and the
WGPU package are the first extension points to evaluate. The repository toolchain
uses Rust 1.98.1. GPUI's crate manifest declares Apache-2.0.

- [Platform selection](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/crates/gpui_platform/src/gpui_platform.rs)
- [Platform interfaces](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/crates/gpui/src/platform.rs)
- [WGPU renderer dependencies](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/crates/gpui_wgpu/Cargo.toml)
- [Rust toolchain](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/rust-toolchain.toml)
- [GPUI package metadata](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/crates/gpui/Cargo.toml)

### Longbridge GPUI Kit, formerly GPUI Component

The repository describes a facade over a behavior foundation and styled component
layer. This is the principal component reuse candidate and a reference for a
coordinated user-facing API. Its published claims about production use and catalog
size are project-reported. ggpui has not independently benchmarked the catalog.
The inspected Kit manifest is version 0.9.1 and gates its platform dependency out
on Android/iOS. Its package declares Apache-2.0, while documentation prose has
additional licensing terms described in the README.

- [Overview and licensing notes](https://github.com/longbridge/gpui-kit/blob/7cd921d493f32ffefc8ddade4c84d781b2a58c23/README.md)
- [Facade package and target gates](https://github.com/longbridge/gpui-kit/blob/7cd921d493f32ffefc8ddade4c84d781b2a58c23/crates/kit/Cargo.toml)

GPUI Fast was also reviewed as a possible future performance reference. Its README
describes it as experimental. Selecting another engine fork is deferred until
baseline measurements justify that added compatibility surface.
[GPUI Fast](https://github.com/longbridge/gpui-fast)

## Rust Android integration and tooling

### User-supplied mobile architecture design

The user supplied an architecture proposal during planning. Its recommendations
were incorporated into the runtime/view/surface/resource split, system-main-thread
ownership, typed JNI host, raw touch forwarding, UTF-16 text editing, AccessKit
reuse, and staged native-view integration. Its proposed first-phase iOS delivery
was scheduled for a later track to follow the user's earlier Android-first scope.
The supplied document is conversation input. The resulting engineering decisions
are recorded in PROJECT_PLAN.md and ARCHITECTURE_DECISIONS.md.

Additional official GPUI sources were inspected to check that these extension
points exist. Existing source remains subject to build and device validation.

- [Portable gesture processing](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/crates/gpui/src/gestures.rs)
- [Existing accessibility integration](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/crates/gpui/src/_accessibility.rs)
- [Official iOS backend](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/crates/gpui_ios/src/gpui_ios.rs)
- [iOS application entry and lifecycle](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/crates/gpui_ios/src/ios/application.rs)
- [Apple package metadata](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/crates/gpui_apple/Cargo.toml)
- [Existing CosmicText implementation](https://github.com/zed-industries/zed/blob/f8cfc7ed9e57503def9479138894f3139f5400cc/crates/gpui_wgpu/src/cosmic_text_system.rs)

### android-activity

The README documents lifecycle/event glue, native-thread execution, Java main
thread callbacks, and activity recreation. It recommends Gradle plus cargo-ndk.
It describes GameActivity's input-method support and the limitations of basic
NativeActivity text input. Match the crate and Android GameActivity library
versions explicitly. Observe its warning about linking duplicate GameActivity
glue through prefab.
[Inspected README](https://github.com/rust-mobile/android-activity/blob/b4ddf059b77be12cdb955d394e65a92e7568d936/README.md)

### cargo-ndk

The tool prepares cross-compilation environment variables and ABI-specific
`jniLibs` layout. Its README lists Windows, macOS, and Linux hosts and documents
NDK selection with `ANDROID_NDK_HOME`. crates.io reported stable version 4.1.2
during planning. Its project offers MIT or Apache-2.0 licensing.

- [Inspected README](https://github.com/bbqsrc/cargo-ndk/blob/8cffdefbc9fc29b15ef50390b73258032b26734b/README.md)
- [Published versions](https://crates.io/crates/cargo-ndk)

### Makepad

Makepad is a reference for an integrated Rust application and mobile toolchain.
Its Android command handler demonstrates SDK setup, device selection, APK/AAB
commands, manifest configuration, and signing concerns. Borrow workflow ideas
while preserving the GPUI application model. Rendering and framework internals
have their own architecture and require separate integration decisions.

- [Project and development workflow](https://github.com/makepad/makepad/blob/506ec05105b9adb08d1d21f9879df181b5244ea7/README.md)
- [Android tooling](https://github.com/makepad/makepad/blob/506ec05105b9adb08d1d21f9879df181b5244ea7/tools/cargo_makepad/src/android/mod.rs)

## Mobile bridges and updates

### Flutter

Use Flutter's host/engine boundary, text input, lifecycle, accessibility, and
platform-view trade-offs as architecture references. GPUI retains responsibility
for its Rust view model, layout, scene, and rendering behavior.

- [Architectural overview](https://docs.flutter.dev/resources/architectural-overview)
- [Android platform views](https://docs.flutter.dev/platform-integration/android/platform-views)
- [Android surface destruction contract](https://developer.android.com/reference/android/view/SurfaceHolder.Callback)
- [Android text editing protocol](https://developer.android.com/reference/android/view/inputmethod/InputConnection)

### Tauri

Tauri's mobile plugin implementation is a reference for Rust/native boundaries.
Its updater documents a check/download/install workflow and supports Windows,
macOS, and Linux. Android and iOS are marked unsupported. Reusing ideas from that
workflow still requires ggpui-specific APIs and Android channel providers.

- [Mobile plugin bridge](https://github.com/tauri-apps/tauri/blob/66b29d1b03bdc08e7275f8da40cc05432a50e1f4/crates/tauri/src/plugin/mobile.rs)
- [Updater platform table and workflow](https://github.com/tauri-apps/plugins-workspace/blob/9aa4f717152cdeceed4bada9c849ad68a9dc402c/plugins/updater/README.md)
- [Updater protocol documentation](https://v2.tauri.app/plugin/updater/)

## Official build and distribution documentation

- [mise Rust management](https://mise.jdx.dev/lang/rust.html)
- [mise Java management](https://mise.jdx.dev/lang/java.html)
- [mise cargo backend](https://mise.jdx.dev/dev-tools/backends/cargo.html)
- [Android NDK downloads and current LTS](https://developer.android.com/ndk/downloads)
- [Android 16 KB page-size requirements and verification](https://developer.android.com/guide/practices/page-sizes)
- [Android GameActivity integration](https://developer.android.com/games/agdk/game-activity/get-started)
- [Google Play in-app updates](https://developer.android.com/guide/playcore/in-app-updates)
- [Android PackageInstaller](https://developer.android.com/reference/android/content/pm/PackageInstaller)
- [Gradle Wrapper and checksum verification](https://docs.gradle.org/current/userguide/gradle_wrapper.html)
- [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0.txt)

Official documentation links are living sources. Recheck policy and tool-version
requirements when implementing and releasing each milestone.
