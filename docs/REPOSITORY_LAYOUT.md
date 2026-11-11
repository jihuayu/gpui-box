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
components/                 Reusable UI packages, themes, and adaptive widgets
examples/
  desktop/                  Desktop application using the facade
platforms/                  Native hosts and packaging resources by platform
vendor/
  gpui/                     Copied official GPUI and its local dependency closure
    crates/                 Engine and existing desktop/mobile backends
    assets/fonts/           Fonts and retained licenses for upstream examples/tests
    Cargo.toml              Reduced GPUI source workspace
    Cargo.upstream.toml     Original official workspace manifest
    UPSTREAM.json           Source revision, adaptations, and file hashes
docs/                       Architecture, roadmap, research, and contributor guidance
```

## Ownership rules

- `vendor/gpui` owns upstream engine and backend code, including the future
  `gpui_android` backend. Keep imported crates together at one revision.
- `crates` owns distribution APIs, application services, update delivery, and CLI
  tooling. Add packages when real implementation begins.
- `components` owns reusable controls and themes. Platform-adaptive behavior lives
  here when it can use the common facade and services.
- `platforms` owns OS host projects, resources, signing, and package integration.
- `examples` contains executable consumer applications and acceptance galleries.
- Application templates will live under `templates` when project generation is
  implemented. Build orchestration will live under `tooling` when needed.

## Current build paths

`mise run check` validates the dependency-light distribution core and CLI.
`mise run check:desktop`, `build:desktop`, and `dev:desktop` use the real copied
GPUI backend for the current desktop OS. GUI builds require that platform's native
dependencies. The `desktop` facade feature exposes GPUI types as `gpui_box::gpui`
and the native application constructor as `gpui_box::application`.

Use `mise -E android install` and `mise -E android run setup:android` to provision
the optional Android environment. Android host/runtime implementation remains
tracked by the Android platform issues. Existing iOS source is included with the
upstream snapshot and requires Apple build infrastructure for validation.
