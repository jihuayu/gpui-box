# M0 Android vertical slice

## Status on 2026-11-11

The platform skeleton and minimal runnable example are implemented. The shared
Rust page runs on Windows x86_64 and the Android x86_64 emulator. Both Android
ARM64 and x86_64 native libraries are packaged in the debug APK. M0 remains open
until ARM64 hardware and a real Chinese IME candidate-selection flow are tested.

## Implementation

| Area | Owning source |
| --- | --- |
| GPUI engine and WGPU/CosmicText renderer | `vendor/gpui` |
| Android Platform/Window/Dispatcher and retained runtime | `crates/gpui-android` |
| Public Android exports | `crates/gpui-box`, feature `android` |
| Shared Chinese/counter/input page | `examples/hello` |
| Desktop entry point | `examples/desktop` |
| Android cdylib and JNI entry points | `examples/android` |
| Kotlin Activity/SurfaceView/InputConnection and APK | `platforms/android` |

The initial repository plan placed the future Android backend under the copied
source workspace. M0 keeps the new backend in distribution-owned `crates` while
the host contract is developed. Upstream GPUI still owns the core, trait
definitions, priority queue, portable gestures, and renderer. The only engine
patch enables Android compilation/export of the existing priority queue.

## Source and dependency identities

- Official Zed baseline is `f8cfc7ed9e57503def9479138894f3139f5400cc`.
  Hash verification found 410 matching original entries and one intentional
  Android gate patch in `crates/gpui/src/gpui.rs`. All 27 copied package manifests
  remain in the reduced upstream workspace. See
  [downstream changes](../vendor/gpui/DOWNSTREAM_CHANGES.md).
- WGPU uses upstream's Git revision
  `cddb2f5d80dc0009b2ef6767635140cacf90a007`, version 30.0.0. CosmicText is 0.19.0.
  The scheduler retains upstream's async-task patch at
  `015845a19fc31752c02367567d3b8900a406f9da`. Root `Cargo.lock` records the graph.
  The Android GPU context supplies a display-only provider because WGPU retains
  that provider for the instance lifetime. Native surface ownership stays with
  the current surface session and never with the process-wide display provider.
- JNI is 0.21.1 and ndk is 0.9.0. The low-level backend and example JNI crate
  permit documented unsafe FFI boundaries. Other distribution crates inherit
  `unsafe_code = "forbid"`.
- Original contributions and copied GPUI source use Apache-2.0. The example
  adapts the Apache-2.0 upstream input example and retains attribution. The
  unmodified bundled Noto Sans CJK SC font uses SIL OFL 1.1. Its SHA256 is
  `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`.
  GPUI Kit is absent from this dependency graph.

The exact Rust/JDK/Gradle/AGP/Kotlin/SDK/NDK tuple is recorded in the
[Android host guide](../platforms/android/README.md). The wrapper JAR SHA256 is
`81a82aaea5abcc8ff68b3dfcb58b3c3c429378efd98e7433460610fecd7ae45f`.

## Validation results

| Check | Result |
| --- | --- |
| `mise run check` | Passed formatting, 3 host tests, core/CLI/backend Clippy, and whitespace checks |
| `cargo build --locked -p gpui-box-example-desktop` | Passed on Windows x86_64 MSVC |
| Desktop visual check | Same Chinese/counter/input page displayed in a Windows GPUI window |
| Desktop/shared/backend Clippy with `-D warnings` | Passed |
| Android x86_64 compile and Clippy with `-D warnings` | Passed |
| `mise -E android run build:android` | Passed ARM64 + x86_64 native build and Gradle debug APK assembly |
| `mise -E android run test:android` | Passed 1 connected instrumentation test |
| Raw Android touch | Counter changed from 0 to 1 |
| Android text and keyboard | Field focused, soft keyboard appeared, text updated visibly |
| Chinese composing protocol | `ni` to `你好` replacement, commit, selection, marked range, and text queries passed against the Rust model |
| UTF-16 and supplementary characters | Emoji selection/replacement and code-point deletion passed |
| Stale generation | Old input rejected in instrumentation, stale attach/detach rejected in host unit test |
| Rotation / Activity recreation | Counter 1 and text `lifecycle` survived portrait-to-landscape recreation |
| Background / resume / return to portrait | State survived, Surface generations advanced through 1–5, no app panic or stale-surface error observed |

The connected test invokes the actual Kotlin InputConnection, JNI, GPUI focused
field, and Rust text model. It checks the composing protocol directly. It does
not exercise a third-party Chinese keyboard's candidate UI. The bundled font
covers the Chinese page. Color emoji font fallback is outside this slice.

Host tests include two existing CLI tests and the new surface-generation test.
Desktop visual verification did not include a complete desktop input regression
suite. Upstream Windows linker informational warnings remain visible during
builds. Initial SDK processing emitted the XML schema-version warning recorded
in the host guide.

## Emulator

| Property | Observed value |
| --- | --- |
| Host | Windows x86_64, WHPX |
| Android Emulator | 37.3.9, build 16863232 |
| Image | Android 15 / API 35, default x86_64, revision 2 |
| AVD device profile | Pixel 6, 1080 × 2400, density 2.625 |
| Graphics mode | `-gpu host` |
| WGPU backend | Vulkan |
| Adapter | NVIDIA GeForce GTX 1650 Ti |
| Driver | NVIDIA 571.96 |

The initial SwiftShader emulator process exited during install/startup. The
host-GPU configuration completed rendering and lifecycle checks. WGPU reports
the emulator's missing `SURFACE_VIEW_FORMATS` downlevel capability, then selects
the adapter after its surface configuration test succeeds.

## Reproduction and evidence

```sh
mise run check
mise run build:desktop
mise run dev:desktop
mise -E android run build:android
mise -E android run test:android
mise -E android run install:android
adb shell am start -W -n dev.gpui.box.example/dev.gpui.box.MainActivity
adb logcat -s gpui-box AndroidRuntime RustStdoutStderr
```

Use the counter and type in the field. Rotate, press Home, reopen the same task,
and rotate back. The counter and text should remain unchanged. Observe paired
surface detach/attach messages and increasing generations in logcat.

Build output and screenshots remain local under ignored paths. APK output is
`platforms/android/app/build/outputs/apk/debug/app-debug.apk`. Connected results
are in `platforms/android/app/build/outputs/androidTest-results/connected`.
Manual screenshots are in `target/m0-evidence`, including `android-rotated.png`
and `android-resumed.png`.

## Renderer decision and remaining acceptance

Proceed with the existing WGPU renderer and CosmicText for the next Android
platform work. The emulator demonstrates Vulkan rendering, atlas retention
across surface replacement, Chinese font loading, and GPUI raw-touch routing.
No additional renderer is required for this slice. HCPP/AHardwareBuffer
composition remains a separate later milestone.

The next acceptance run must use an ARM64 device, exercise a Chinese keyboard's
real composing/candidate/commit flow, and validate the chosen minimum API level.
Process-death restoration, multi-field IME sessions, animated edge-to-edge
insets, accessibility, native services, and GPU device-loss recovery remain
explicit follow-up platform work. Existing milestone estimates remain planning
ranges pending those hardware results.
