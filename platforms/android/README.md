# Android host

This host embeds the shared Rust page from `examples/hello` in a Kotlin
`Activity` and `SurfaceView`. `examples/android` exports typed JNI entry points
through the `gpui-box` Android facade. `crates/gpui-android` implements the GPUI
platform, display, window, dispatcher, native surface, and text-input bridge.

## Toolchain

| Tool | Pinned version |
| --- | --- |
| Rust | 1.98.1 |
| JDK | Temurin 17.0.21+9 |
| cargo-ndk | 4.1.2 |
| Gradle | 8.13 |
| Android Gradle Plugin | 8.10.1 |
| Kotlin Android plugin | 2.1.20 |
| Compile SDK | 36 |
| Build tools | 36.0.0 |
| NDK | 30.0.16248370 |
| JNI crate | 0.21.1 |
| Rust NDK crate | 0.9.0 |
| Minimum / target API | 26 / 35 |
| Packaged ABIs | arm64-v8a, x86_64 |

API 26 is the configured native build floor. Runtime validation currently covers
API 35. Minimum-API device compatibility still requires validation.
The initial SDK tooling emitted an SDK XML schema-version warning. The recorded
build and instrumentation run succeed with this tuple.

## Build and run

Install Google's Android SDK and accept the required licenses. Install
`platforms;android-36`, `build-tools;36.0.0`, `ndk;30.0.16248370`, and
`platform-tools`. Set `ANDROID_HOME` to the SDK directory, or set `sdk.dir` in the
ignored `platforms/android/local.properties` file. The Gradle task selects the
pinned NDK inside that SDK.

Run from the repository root.

```sh
mise -E android install
mise -E android run setup:android
mise -E android run build:android
mise -E android run install:android
adb shell am start -n dev.gpui.box.example/dev.gpui.box.MainActivity
mise -E android run test:android
```

`build:android` invokes Cargo for both ABIs and builds a debug APK at
`platforms/android/app/build/outputs/apk/debug/app-debug.apk`. Cargo uses the root
lockfile. The app's native library is `libgpui_box_android.so`. This debug APK
uses Android's development signing key.

The checked-in Gradle wrapper also pins 8.13 and validates the distribution SHA256.
Use `mise -E android exec -- platforms/android/gradlew.bat -p platforms/android
:app:assembleDebug` on Windows, or `mise -E android exec -- sh
platforms/android/gradlew -p platforms/android :app:assembleDebug` on Unix.

For an x86_64 emulator, install `emulator` and
`system-images;android-35;default;x86_64`, create an AVD, and start it with Vulkan
support. This workstation passed with `-gpu host` and WHPX. Its initial
SwiftShader emulator process exited during app installation/startup, so that
configuration remains unverified.

## Runtime contracts

- The Android main Looper owns GPUI App/Entity state and initial GPU submission.
  Android calls `run_embedded`, retains its `ApplicationHandle`, and schedules
  foreground work with Handler and Choreographer.
- Runtime, Kotlin view attachment, surface generation, and renderer resources
  have separate lifetimes. Activity recreation reuses the process-owned runtime.
- `surfaceDestroyed` synchronously unconfigures the WGPU surface, waits for
  outstanding device work, and releases the native surface reference. Reattach
  uses the original WGPU instance/device and retains the glyph atlas.
- Surface, input, and resize callbacks carry increasing process-wide generation
  IDs. Old IDs cannot mutate the replacement surface's state. A resize of the
  existing Surface retains its generation.
- Kotlin sends raw touch contacts with lifetime-unique IDs. GPUI's existing
  recognizer dispatches tap and pan interactions.
- Rust owns text, selection, and composing ranges. InputConnection exchanges
  UTF-16 offsets and queries Rust for text. Java host requests post work before
  returning to native code. Callback slots release their borrow before entering
  GPUI application code.
- The M0 host fits the SurfaceView inside system bars and keyboard insets. Full
  edge-to-edge safe-area and animated GPUI WindowInsets delivery are future work.

## Current boundaries

This is a single-window platform skeleton. The display reports a provisional
60 Hz refresh interval. Background timers use individual sleeping threads.
Clipboard, credentials, file pickers, URL intents, accessibility, system back
navigation integration, memory-pressure handling, GPU device-loss recovery,
process-death persistence, and complete IME configuration/session switching
remain platform work. Runtime state survives Activity recreation within a live
process. A new process starts with the example's initial state.

See [M0 validation](../../docs/M0_ANDROID_VALIDATION.md) for commands, results,
renderer decision, and outstanding device acceptance.
