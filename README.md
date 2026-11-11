# gpui-box

A coordinated GPUI distribution for desktop and mobile applications, with shared
components, application services, developer tools, packaging, and platform-aware
updates.

[Project repository](https://github.com/jihuayu/gpui-box)

## Platform scope

Windows, macOS, Linux, Android, and iOS are managed in this repository. Desktop
and mobile share the same official GPUI baseline and distribution APIs. Android
is the first new mobile backend. Existing desktop and iOS backend source is
included in the vendored upstream snapshot.

## Repository structure

| Directory | Responsibility |
| --- | --- |
| `vendor/gpui` | Copied official engine, platform backends, local dependency closure, assets, and provenance |
| `crates` | Distribution facade, CLI, and future application/update services |
| `components` | Reusable controls, themes, controller models, and adaptive UI packages |
| `examples` | Executable applications and acceptance galleries |
| `platforms` | Native host projects, OS resources, signing, and packaging |
| `docs` | Architecture, implementation plans, research, and repository guidance |
| `templates` | Application-owned starter source and mise configuration for project generation |

See [repository layout and ownership rules](docs/REPOSITORY_LAYOUT.md).

## Development

Install [mise](https://mise.jdx.dev/getting-started.html), then run from the root.

```sh
mise install
mise run setup:rust
mise run doctor
mise run check
mise run cli
```

Rust 1.98.1 is pinned in `mise.toml`. The root Cargo workspace contains the
`gpui-box` facade, CLI, Android backend, and shared/desktop/Android examples. `Cargo.lock` records
the coordinated dependency resolution. Project-owned Rust uses Apache-2.0,
English identifiers, and shared package metadata.

### Desktop

```sh
mise run check:desktop
mise run build:desktop
mise run dev:desktop
```

Enable the facade's `desktop` feature to use `gpui_box::gpui` and
`gpui_box::application`. Desktop builds use path dependencies on the checked-in
GPUI source. Windows requires MSVC C++ Build Tools and a Windows SDK. macOS
requires Xcode and the Metal toolchain. Linux requires the selected X11/Wayland,
font, graphics, and system development libraries described by upstream GPUI.
The facade currently enables both X11 and Wayland for desktop builds.

### Android

```sh
mise -E android install
mise -E android run setup:android
mise -E android run doctor:android
mise -E android run build:android
mise -E android run install:android
mise -E android run test:android
```

The optional Android environment pins Temurin JDK 17.0.21+9 and cargo-ndk 4.1.2.
Gradle 8.13 is also managed by mise. Android Studio or Google's command-line
tools manage SDK/NDK installation and license acceptance. Set `ANDROID_HOME` to
the SDK directory. The Kotlin SurfaceView host calls the GPUI main-thread runtime
through typed JNI and renders the same Rust page as the desktop example.
Android requires API 35 or newer. The current phase uses emulator validation
and cross-builds both ABIs, with physical-device qualification deferred.
Enable the facade's `android` feature for `gpui_box::android`.
See [Android setup](platforms/android/README.md) and
[M0 validation](docs/M0_ANDROID_VALIDATION.md).

## Implementation status

M0 is complete against the API 35 emulator gate. M1 provides focus-scoped input
connections, nested batch edits, clipboard, memory warnings, guarded reusable JNI
entrypoints, EditorInfo/actions, cursor-anchor geometry, predictive-back transport
and an initial AccessKit virtual-node adapter. The desktop and Android examples
share Chinese text, editable fields, reusable components and an adaptive gallery.
The CLI now generates source-checkout applications and provides doctor,
dev/build/install, debug/release Android packaging, Windows portable development
bundles, device selection and logs. See the reports
below for tested behavior and open M1–M3 acceptance gates. Application services,
release qualification and update providers have their own roadmap issues.

### Generate an application

Build the CLI with `mise run build`. Add the resulting `target/debug` directory
to PATH, then use an absolute path to this source checkout.

```sh
gpui-box new "My Application" --distribution /absolute/path/to/gpui-box
cd "My Application"
mise trust
mise trust mise.android.toml
mise install
mise -E android install
mise exec -- rustup target add aarch64-linux-android x86_64-linux-android
gpui-box doctor
gpui-box dev desktop
gpui-box dev android --device emulator-5554
```

Configure identity, version, assets, targets and the development channel in
`gpui-box.json`. Keep source in `app/src` and native customizations in
`android-custom.gradle.kts`. Read [the CLI workflow](docs/M3_DEVELOPER_WORKFLOW.md)
before regeneration or distributing generated output.

## Documentation

- [Repository layout](docs/REPOSITORY_LAYOUT.md)
- [Project plan](docs/PROJECT_PLAN.md)
- [Architecture decisions](docs/ARCHITECTURE_DECISIONS.md)
- [M1 Android validation](docs/M1_ANDROID_VALIDATION.md)
- [M2 component gallery](docs/M2_COMPONENT_GALLERY.md)
- [M3 developer workflow](docs/M3_DEVELOPER_WORKFLOW.md)
- [CLI command reference](docs/CLI_REFERENCE.md)
- [M4 application packaging](docs/M4_ANDROID_PACKAGING.md)
- [M5 update trust and orchestration](docs/M5_UPDATE_CONTRACTS.md)
- [Reference research](docs/REFERENCES.md)
- [Later Android native composition](docs/ANDROID_COMPOSITION_PLAN.md)
- [GitHub roadmap](https://github.com/jihuayu/gpui-box/issues/17)

## License

Original gpui-box contributions are licensed under Apache License, Version 2.0.
See [LICENSE](LICENSE). Imported source, fonts, and dependencies retain their
licenses and attribution. [Upstream provenance](vendor/gpui/README.gpui-box.md)
records the copied source revision and workspace adaptation.
