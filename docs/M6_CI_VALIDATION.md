# M6 continuous integration qualification

## Windows shared-code gate

`.github/workflows/windows.yml` runs on pull requests, pushes to main and manual
dispatch. The Windows 2025 hosted image provides MSVC and the Windows SDK. The job
uses immutable revisions of checkout, MSVC setup, mise setup and artifact upload.
Mise 2026.7.5 is checked against the published Windows x64 SHA-256, and `mise.toml`
selects Rust 1.98.1. Rustfmt and Clippy are installed through the existing mise
setup task. The workflow records rustc's verbose identity and Cargo's version.

The job invokes existing repository entry points in order

| Entry point | Scope |
| --- | --- |
| `mise run check` | Formatting, shared/CLI/system package tests, compile-fail doctests, strict all-target Clippy and diff checks |
| `mise run check:components` | Component model/context tests and strict component/gallery Clippy |
| `cargo test --locked -p gpui-box-example-hello` through mise | Shared gallery and typed workflow tests |
| `mise run check:desktop` | Windows desktop example compilation against the copied GPUI identity |

Repository permissions are read-only. Checkout credentials are discarded. The job
uses no release signing/deployment secrets. Superseded runs for the same workflow
and ref are cancelled. The timeout is 60 minutes. Logs and tool identities under
`target/ci` are uploaded on success or failure and retained for 14 days. An early
setup failure remains visible in the Actions job logs even if that directory has
not been created.

## Qualification boundary

Local Windows checks pass 196 host tests plus six compile-fail doctests and two
nested child reports, 16 component tests and six gallery/workflow tests. The local
API 35 x86_64 emulator suite passes 21/21 after restarting an unresponsive emulator.
Those emulator checks remain separate from this hosted Windows job.

The first hosted pull-request run passed all steps in 8 minutes 37 seconds,
using upstream Rust 1.98.1 and the hosted Windows toolchain. It passed the same
196 shared/CLI tests, six compile-fail doctests, 16 component tests, six gallery
tests, strict lints and desktop compilation. The validation-log artifact was
uploaded successfully. GitHub reported a Node 20 deprecation annotation for
the pinned mise/MSVC/artifact actions and forced those actions onto Node 24.
Their steps completed successfully in this run.

[Hosted qualification run](https://github.com/jihuayu/gpui-box/actions/runs/43033815129)

[Validation log artifact](https://github.com/jihuayu/gpui-box/actions/runs/43033815129/artifacts/13862158447)

This first job covers shared code, CLI, components and desktop compilation.
Desktop interactive/screen-reader sessions, macOS build coverage, Android
cross-build/emulator CI, feature/dependency audit matrices, release signing,
16 KB runtime pages, GPU/performance and hardware qualification retain separate
M6 gates. Physical-device testing remains deferred under the current project
scope. M6 stays open until its full release acceptance is satisfied.

## Linux shared-code gate

`.github/workflows/linux.yml` runs the same shared/CLI, component, gallery and
desktop compilation entry points on Ubuntu 24.04. The CLI suite includes its
Unix-specific managed-output and metadata symlink regression on this runner.
The workflow pins the same mise/Rust versions, verifies the Linux x64 mise
SHA-256 and retains APT, tool-identity and validation logs for 14 days.

Native prerequisites cover Clang, CMake, pkg-config, OpenSSL, udev, Fontconfig,
XKB, X11, Wayland, Vulkan/EGL, ALSA, GLib and D-Bus. Their Ubuntu package versions
follow the hosted image and current official Ubuntu repositories. This job
qualifies the tested image/package tuple. The release SDK closure remains
tracked separately under #62.

The first attempt failed during APT provisioning before Rust setup. Its mirror
discovery repeatedly reported DNS failure for azure.archive.ubuntu.com. A partial
index update returned success and package installation continued against stale
indexes until it failed after roughly 45 minutes. The corrected workflow uses
explicit official Ubuntu HTTPS sources for this command, treats incomplete
index updates as failures, bounds retries/download timeouts and limits the
provisioning step to ten minutes. Logs are created before installation so a
provisioning failure can still produce an artifact. The repository's normal
developer APT configuration is outside this hosted workflow.

A subsequent attempt confirmed DNS failures for archive.ubuntu.com and
security.ubuntu.com directly. It failed in two minutes and uploaded its APT log.
The workflow records DNS observations and retries index retrieval through the
kernel.org Ubuntu mirror if the primary update fails. Ubuntu's signed package
indexes and normal APT verification remain enabled for that fallback.

[Initial provisioning failure](https://github.com/jihuayu/gpui-box/actions/runs/43036937740)

The final PR #91 head passed every Linux step in 7 minutes 21 seconds and every
Windows step in 8 minutes 27 seconds. Linux ran the Unix managed-output/metadata
symlink regression, shared tests/lints, component checks, six gallery tests and
native desktop compilation. A surfaced existing storage regression also exposed
Windows-root path spellings being accepted as relative names on Unix.
`AppDirectory::child` now rejects drive-qualified and backslash-rooted inputs
consistently on every host. Its expanded tests retain accepted nested paths.
PR #91 merged as `40c52f3`. Linux interactive rendering, desktop service-provider behavior, assistive technology
and packaging/update support retain their operation-specific acceptance gates.
Android cross-build/emulator CI and macOS qualification remain separate work.

[Passing Linux qualification](https://github.com/jihuayu/gpui-box/actions/runs/43047699099)

[Passing Windows qualification](https://github.com/jihuayu/gpui-box/actions/runs/43047699123)

[Linux CI implementation and evidence](https://github.com/jihuayu/gpui-box/pull/91)

## Android hosted build gate

`.github/workflows/android.yml` uses Ubuntu24.04 and the existing pinned mise
Android environment. Rust1.98.1, Temurin17.0.21+9, cargo-ndk4.1.2 and the
checksum-configured Gradle8.13 Wrapper build native platform35 for ARM64/x86_64.
The hosted image supplies Android platform36, build-tools36.0.0 and NDK30.0.16248370.
The job requires those exact packages and records their source.properties plus
Rust/Cargo/Java/cargo-ndk identities. A missing image prerequisite fails setup.
Hosted image/platform-tools/command-line-tools revisions follow runner updates.
Their qualification applies to the recorded image/package tuple.

The job runs strict native Clippy for backend/example on both ABIs, builds the
debug application and instrumentation APKs, runs Android lint and verifies both
native libraries exist in the APK. It retains logs/lint reports for14days with
read-only repository permissions, immutable action revisions, discarded checkout
credentials and superseded-run cancellation. Hosted execution evidence remains
in the implementation PR until qualification completes. Instrumentation APK
compilation is a build gate. Emulator runtime CI, signing/install/update/release,
16KB runtime, GPU/performance and physical-device acceptance retain separate
gates. Physical-device testing remains deferred.

The first complete hosted Android qualification on source head274a6f2 passed all
steps in7minutes36seconds. Both native targets passed strict Clippy. Gradle ran
all72tasks, built both APKs and completed lint. APK inspection found the ARM64
and x86_64 libraries. The33441-byte validation/report artifact uploaded
successfully. GitHub forced pinned Node20 actions onto Node24 and their steps
passed. The same source head also passes Linux shared/component/gallery/desktop
checks. Final documentation-head checks are recorded in the implementation PR.

[Hosted Android qualification](https://github.com/jihuayu/gpui-box/actions/runs/43057400731)

[Android validation reports](https://github.com/jihuayu/gpui-box/actions/runs/43057400731/artifacts/13872052434)

## External SDK and generated applications

All three jobs run `mise run check:templates` after their repository checks.
Linux/Windows select desktop and Android selects Android. The opt-in integration
test drives the real CLI to export/verify committed HEAD, generates both
starter/adaptive-v1 across three target sets in external spaces/Chinese paths,
and verifies no-op generation plus retained source/native customizations. Four
enabled combinations build through the normal CLI. Disabled targets must fail
planning. Final SDK verification checks that those builds preserved the source
closure. Desktop executables and both native APK ABIs are inspected, and
artifact SHA-256 values are recorded.

The existing14-day validation artifact includes per-command stdout/stderr,
version/provenance/build receipts and explicit runtime-execution status. The
fresh workspace and isolated Cargo outputs live outside the checkout and remain
available for diagnosis during the hosted job. Normal tool/dependency downloads
use inherited caches/network settings. Hosted image packages remain part of the
recorded qualification tuple. A template-build receipt records compilation and
artifact evidence. Interactive, signed release/install/update, offline bootstrap,
registry/native CLI publication and physical-device gates remain independent.

[Local entry point and report contract](M3_DEVELOPER_WORKFLOW.md#reproducible-external-template-qualification)
