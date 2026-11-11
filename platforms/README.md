# Platform hosts

Native host projects and platform packaging resources belong here.

| Directory | Responsibility |
| --- | --- |
| `windows` | Windows host resources, signing, and installer integration |
| `macos` | macOS bundle, signing, and notarization integration |
| `linux` | Linux desktop integration and package formats |
| `android` | Kotlin/JNI host, Gradle project, and Android packages |
| `ios` | UIKit host project, signing, and application packages |

Create each directory with its first implementation. GPUI backend source remains
under `vendor/gpui/crates`. Shared distribution services belong under `crates`.
