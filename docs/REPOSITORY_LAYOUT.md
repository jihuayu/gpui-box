# Repository layout

gpui-box manages a single GPUI distribution for desktop and mobile applications.
Windows, macOS, Linux, Android, and iOS share the same engine source, facade,
component system, application services, and developer tools.

```text
Cargo.toml                  Distribution workspace and shared package metadata
Cargo.lock                  Coordinated application/tool dependency resolution
mise.toml                   Shared Rust tools and repository tasks
mise.android.toml           Optional Android JDK and cargo-ndk environment
crates/
  gpui-box/                 Public facade and coordinated GPUI exports
  gpui-box-cli/             Distribution development tools
  gpui-android/             Distribution-owned Android platform integration
  gpui-system/              Typed capability, request and event contracts
  gpui-app-data/            Private directories and recoverable settings
  gpui-permissions/         Permission status, contextual requests, settings recovery
components/
  gpui-box-components/      Controls, themes, screen-owned models and shell primitives
examples/
  desktop/                  Desktop application using the facade
  hello/                    Shared Rust view and bundled OFL font
  android/                  Android example cdylib and typed JNI exports
platforms/                  Native hosts and packaging resources by platform
  android/                  Kotlin SurfaceView host and Gradle APK project
vendor/
  gpui/                     Copied official GPUI and its local dependency closure
    crates/                 Engine and existing desktop/mobile backends
    assets/fonts/           Fonts and retained licenses for upstream examples/tests
    Cargo.toml              Reduced GPUI source workspace
    Cargo.upstream.toml     Original official workspace manifest
    UPSTREAM.json           Source revision, adaptations, and file hashes
docs/                       Architecture, roadmap, research, and contributor guidance
templates/                  Shared app source, mise environments and starter instructions
```

## Ownership rules

- `vendor/gpui` owns copied upstream engine and backend code. Keep imported
  crates together at one revision and record source patches in
  `DOWNSTREAM_CHANGES.md`.
- `crates` owns distribution APIs, application services, update delivery, and CLI
  tooling. The new `gpui-android` backend lives here while its distribution-owned
  JNI/SurfaceView contract is developed. This refines the initial plan to place
  it inside the upstream snapshot.
- `components` owns reusable controls and themes. Platform-adaptive behavior lives
  here when it can use the common facade and services.
- `platforms` owns OS host projects, resources, signing, and package integration.
- `examples` contains executable consumer applications and acceptance galleries.
- `templates` owns starter application source and mise configuration. Generator
  policy, managed manifests and recovery state live in `crates/gpui-box-cli`.
  Build orchestration will live under `tooling` when needed.

## Current build paths

`mise run check` validates the dependency-light distribution core and CLI.
`mise run check:components` validates component models and shared gallery lints.
`mise run check:desktop`, `build:desktop`, and `dev:desktop` use the real copied
GPUI backend for the current desktop OS. GUI builds require that platform's native
dependencies. The `desktop` facade feature exposes GPUI types as `gpui_box::gpui`
and the native application constructor as `gpui_box::application`.

Use `mise -E android install` and `mise -E android run setup:android` to provision
the optional Android environment. `mise -E android run build:android` builds the
two-ABI APK and `test:android` runs connected instrumentation tests. See
`platforms/android/README.md`. Existing iOS source is included with the
upstream snapshot and requires Apple build infrastructure for validation.
