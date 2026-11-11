# gpui-box

A developer-focused GPUI distribution with Android support, reusable components,
application tooling, packaging, and platform-aware updates.

[Project repository](https://github.com/jihuayu/gpui-box)

## Project status

The repository currently contains the project design and development-tool
configuration. The first implementation milestone is an Android feasibility
prototype. Runtime crates, Android projects, and distributable applications are
planned deliverables.

The first release targets Android, with Windows as the initial desktop development
and regression platform. Additional desktop platforms and iOS follow later.

## Direction

- Preserve GPUI's Rust-native programming model and GPU-rendered interface.
- Provide a coordinated dependency set through a small application-facing facade.
- Build platform support on official Zed GPUI and reuse compatible GPUI Kit layers.
- Add touch-friendly application patterns with a thin native Android host.
- Support the complete Android workflow from project creation to signed packages.
- Provide explicit update strategies for desktop, Google Play, and direct APK delivery.
- Keep platform capabilities and compatibility limits visible to application authors.

## Development environment

Install [mise](https://mise.jdx.dev/getting-started.html), then run these commands
from the repository root.

```sh
mise install
mise run setup:rust
mise run doctor
mise run check
```

The project pins Rust 1.98.1, Temurin JDK 17.0.21+9, and cargo-ndk 4.1.2 in
`mise.toml`. Rust follows the GPUI upstream toolchain inspected during planning.
Android cross-compilation targets are installed by `setup:rust`. The first Rust
implementation will also use that task to install rustfmt and Clippy.

Rustup, mise, Git, and the host platform's native compiler prerequisites must be
available. The cargo backend may compile cargo-ndk from source. Windows builds
will require Visual Studio C++ Build Tools and a Windows SDK.

Android Studio or Google's command-line tools will manage the Android SDK, NDK,
emulator, and SDK license acceptance. Set `ANDROID_HOME` to the SDK directory.
The Android prototype will pin SDK/NDK versions and a compatible Gradle, Android
Gradle Plugin, and Kotlin tuple. Its checked-in Gradle Wrapper will own Gradle.
See the plan for the proposed baseline and its validation gate.

`mise run doctor` currently reports tool versions and installed Rust targets.
`mise run check` validates mise task definitions and Git whitespace. Both tasks
will grow as implementation milestones introduce buildable packages.

## Documentation

- [Project plan and milestone acceptance criteria](docs/PROJECT_PLAN.md)
- [Architecture decisions](docs/ARCHITECTURE_DECISIONS.md)
- [Reference implementation research](docs/REFERENCES.md)
- [Later Android native composition technical plan](docs/ANDROID_COMPOSITION_PLAN.md)

Project documentation, code, configuration, and public API names use English.
Planned Cargo packages and the CLI use the `gpui-box` prefix. Rust imports use
`gpui_box` for the facade, following Cargo's hyphen-to-underscore convention.

## License

Original gpui-box contributions are licensed under Apache License, Version 2.0.
Dependencies and any future vendored source retain their respective licenses and
required notices. See [LICENSE](LICENSE).
