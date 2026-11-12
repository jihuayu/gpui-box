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
test drives the real CLI to export/verify committed HEAD, generates
starter/adaptive/component/workbench-v1 across three target sets in external spaces/Chinese paths,
and verifies no-op generation plus retained source/native customizations. Eight
initially enabled combinations build through the normal CLI. Disabled targets
must first fail planning. The workbench case then previews/adds the missing target,
verifies repeated addition has no writes, and builds it through the same CLI.
This ninth build qualifies platform addition in both directions across the host
matrix. Final SDK verification checks that those builds preserved the source
closure. Desktop executables and both native APK ABIs are inspected, and
artifact SHA-256 values are recorded. The embedded CLI registry digest must match
the exported SDK snapshot. A combined workbench/component dependency resolution
retains exact registry/source identities in the same receipt.

The existing14-day validation artifact includes per-command stdout/stderr,
version/provenance/build receipts and explicit runtime-execution status. The
fresh workspace and requested Cargo outputs live outside the checkout and remain
available for diagnosis during the hosted job. Cargo metadata resolves the
actual desktop output layout. Normal tool/dependency downloads
use inherited caches/network settings. Hosted image packages remain part of the
recorded qualification tuple. A template-build receipt records compilation and
artifact evidence. Interactive, signed release/install/update, offline bootstrap,
registry/native CLI publication and physical-device gates remain independent.

[Local entry point and report contract](M3_DEVELOPER_WORKFLOW.md#reproducible-external-template-qualification)

## Independent capability feature qualification

```sh
mise run check:features
GPUI_BOX_FEATURE_TARGET=android mise -E android run check:features
```

Set GPUI_BOX_FEATURE_REPORT_DIR to a fresh report directory to repeat a run.
The default is target/ci/feature-qualification. The harness discovers the
facade feature names from current Cargo metadata and requires empty default
features. Each capability and its optional -gpui adapter compiles independently
with --no-default-features. Additional cases cover an empty facade, all contract
features together, the host platform, and all adapters with that platform.
The current facade has 43 capability selections, giving 47 cases per target.
Windows/Linux use rustc's host triple explicitly. Android uses cargo-ndk at API35
for ARM64 and x86_64, giving 94 cases across both ABIs.

Before each compilation, cargo tree --edges normal records the selected target's
runtime dependency graph. Contract selections reject the GPUI engine, renderer
and platform backend packages. Adapted selections require exactly one GPUI
package identity resolving to vendor/gpui/crates/gpui. Renamed gpui-pre packages
fail qualification. Android also rejects unrelated native/Web platform backends.
Only the selected platform is enabled for the combined adapted case. Build and
development dependencies retain their own validation, and this graph inspection
does not evaluate arbitrary future package aliases.

Each case runs cargo check --lib through the pinned mise environment. Tool and
dependency downloads follow normal Cargo/mise cache settings. These checks
validate Rust compilation and dependency selection. Native linking, installation,
provider behavior, assistive-reader interaction and GPU/performance acceptance
use their independent gates. The existing desktop/APK and external-template
build steps continue to provide linking/package evidence.

The schema-v1 receipt records source and upstream revisions, Rust/Cargo identity,
host/target, selected features, normal package identities, commands, exit codes,
durations and completed cases. It records runtime_executed=false and is written
on a qualification error as well. Child stdout/stderr use files so background
helpers cannot retain a capture pipe. All three native jobs run this gate and
retain its logs/receipt with the existing validation artifacts for 14 days.
Actual provider/runtime evidence remains with #70 and the service owners.

## Hosted Android emulator runtime

The Android job starts a fresh API35 Google APIs x86_64 Pixel7 AVD after building
and inspecting both APKs. The emulator runner action uses a fixed commit and KVM
on Ubuntu24.04. Lavapipe supplies software Vulkan rendering. Emulator and
system-image package versions follow the recorded SDK tuple. Animations remain
enabled for the IME/back cases, with snapshots disabled and 4096MiB RAM.
KVM setup waits for udev events, requires the character device, applies access
permissions to the ephemeral runner device and records its effective identity
and permissions before startup.
The SDK package configuration and emulator invocation follow the upstream
runner and Android graphics documentation.

[Pinned emulator runner](https://github.com/ReactiveCircus/android-emulator-runner/tree/a421e43855164a8197daf9d8d40fe71c6996bb0d)

[Android emulator graphics modes](https://developer.android.com/studio/run/emulator-acceleration)

tooling/android/check-emulator.sh accepts only an emulator serial, verifies API35
and x86_64, installs the built application/test APKs and resets example app data
before the full suite. It runs AndroidJUnitRunner with a 12minute client limit.
Its verifier requires the success summary, at least23tests, one normal final
completion code, consistent counts, every ordered start/success pair and unique
test identities. Failed/skipped/truncated/duplicated output fails the gate even
when adb exits successfully. Fixture checks exercise those rejection paths.

The existing validation artifact retains application/test APK SHA256 values,
source revision, Python/emulator/system-image identity, device properties,
page size/density, instrumentation output and a schema-v1 verification receipt.
An exit trap also collects logcat, SurfaceFlinger diagnostics and a screenshot
on failure. Every adb client and diagnostic capture has a timeout plus forced
termination grace so an offline emulator cannot block failure reporting.
Setup/client timeouts retain their available diagnostics without a
successful verification receipt. The runner owns this temporary emulator and
stops it when the action ends. Application-data reset is specific to the example.

This runtime observation covers a software-GPU emulator. TalkBack speech, real
Chinese IME candidate UI, device-loss recovery, vendor-GPU/16KB environments and
physical devices retain their independent M1/M6 acceptance gates. Physical-device
testing remains deferred.
