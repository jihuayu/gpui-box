# M3 developer workflow progress

## Source-checkout application template

Build `gpui-box-cli` using `mise run build`, then add its `target/debug` directory
to PATH. `gpui-box new` requires the distribution source checkout. The generated
app uses path dependencies on this checkout's facade, components and official
GPUI source, sharing one engine identity. Keep that checkout at a known revision.

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
gpui-box build desktop
gpui-box package android
gpui-box devices
gpui-box logs --device emulator-5554
```

Set `ANDROID_HOME` to the SDK. Install platform-tools, platform 36,
build-tools 36.0.0 and NDK 30.0.16248370, then run `sdkmanager --licenses`.
Mise pins Rust 1.98.1, JDK 17.0.21+9, Gradle 8.13 and cargo-ndk 4.1.2.
Windows also requires MSVC C++ Build Tools and a Windows SDK.

## Configuration v1

```json
{
  "schema_version": 1,
  "name": "My Application",
  "application_id": "dev.example.myapp",
  "version": "0.1.0",
  "version_code": 1,
  "distribution": "/absolute/path/to/gpui-box",
  "targets": ["desktop", "android"],
  "update_channel": "development",
  "permissions": [],
  "assets": ["assets"]
}
```

Unknown keys, unsupported targets/channels/permissions, invalid identity/version
and asset parent traversal produce errors. Optional `internet` emits the Android
INTERNET declaration. Other native service declarations await the owning
providers and typed schema integration in #4/#18. App name is emitted through an
escaped Android string resource. All three Rust packages and Android metadata use
the configured version. `version_code` is bounded by Android's publishing limit.
Asset entries are project-relative Android asset roots. The starter font is also
embedded in Rust for desktop/Android text rendering.

The current development template always includes desktop and Android source.
`targets` selects permitted CLI workflows. Android uses minSdk/targetSdk 35 and
compileSdk 36. `package android` creates a debug-key APK. Release-signed APK/AAB
and update channels remain in M4/M5. Desktop installers remain in M7.

## Ownership and regeneration

```text
gpui-box.json               Application configuration
app/src/lib.rs              Application-owned shared view/model
desktop/src/main.rs         Application-owned desktop entry
android/src/lib.rs          Application-owned JNI launch macro
android-custom.gradle.kts   Application-owned Android customization
assets/                    Application assets and font license
android-host/              Generator-managed Android manifests/build settings
.gpui-box/generated.json   Managed-file SHA256 state
.gpui-box/pending.json     Interrupted-generation journal, when present
```

`new` builds a sibling staging directory and publishes it by rename. An existing
destination is rejected. Failed staging is retained for inspection. `generate`
preflights managed hashes, rejects application edits to managed files, writes a
recovery journal, then replaces individual files with synced sibling temporary
files. The journal accepts previous/current generated hashes after interruption.
A create-new lock serializes generators. After a killed process, verify that no
generator is active before removing `.gpui-box/generate.lock`, then rerun
`generate`. Application source and the native customization file are preserved.
The lockfile is seeded from the distribution and reconciled on the first build.
Commit the resulting application lockfile.

CLI build/dev/install/package refresh managed manifests first. Direct mise tasks
use the last generated manifests, so run `gpui-box generate` after config changes.
Regeneration preserves starter mise files. Review updated tool pins when upgrading
the distribution. Multiple build processes should use a stable config snapshot.

Gradle declares Rust manifest/source, app assets, engine/shader, distribution,
mise, NDK, API, profile and ABI inputs. It outputs both native libraries. External
source inputs exclude target/build output. A no-change second build reports
`buildRust UP-TO-DATE`. Features/profile are fixed by this development template.
Feature and release-profile configuration will extend this contract in M4.

## Evidence on November 11, 2026

- CLI integration tests cover paths containing spaces, destination conflicts,
  unknown configuration/channel errors, requested manifest permissions,
  customization preservation, managed conflicts, interrupted write recovery,
  version propagation, Kotlin dollar escaping and asset traversal rejection.
- Generated `target/Generated App QA` built the Windows executable and both
  Android ABIs. `dev android --device emulator-5554` installed and launched it on
  the API 35 Android 15 x86_64 Vulkan emulator. A second `build android` reported
  all 36 tasks up to date. The generated APK and an emulator screenshot were
  inspected locally.
- A fresh `target/Generated Final App` repeated Windows and both-ABI Android
  builds after template finalization. CLI installation and cold launch succeeded
  on the same emulator. Screenshot inspection confirmed `Count 0` to `Count 1`
  and the entered `TemplateQA` text. A second Android build reported all 36 tasks
  up to date. Windows drive and UNC canonical-path normalization have a focused
  regression test. A network-share build remains unqualified.
- Windows runtime inspection verified rendering, pointer count changes, Chinese
  text entry and single Enter activation after rebuilding the current component
  implementation. Physical-device testing remains deferred.
- `doctor` runs tool queries through the application's mise environment and
  checks SDK/NDK paths, Rust targets, assets and license records. On this host it
  correctly reported the missing `licenses/android-sdk-license` record even
  though the preinstalled toolchain built successfully. The diagnostic remained
  failing until license setup is completed.

## Remaining acceptance

M3 stays open in #4 for typed service/provider declarations, URL/document/share
registration, native permission descriptions and provider evidence inspection.
The source-checkout template and debug workflows are implemented. Portable
packaged template distribution, release signing/profile/feature options, broader
host diagnostics and non-Windows template qualification remain separate gates.

## Typed service declaration continuation

Service schema v1 adds permission kinds with printable contextual purpose strings
and lowercase custom URL schemes. Unknown provider objects, versions, duplicate
kinds/schemes, web/resource schemes and unsupported platforms fail validation.
Generation emits requested native permissions, singleTop custom-scheme intent
filters and a managed `gpui-box-services.json` asset. Regeneration removes obsolete
declarations and preserves application source/customization points. Existing
configurations can omit services. The starter README includes the schema example.

Doctor reports configured permissions and schemes separately from runtime evidence.
Permission declarations alone do not install #20's native provider. Document,
notification/share/export provider schemas remain with their integration gates.
Custom URL delivery consumes the generated asset and forwards stable IDs through
an application-owned callback. The host provides bounded transient retries and
process-local deduplication. Durable routing uses #24's inbox on a worker.

CLI tests now total 11, including typed generation/removal and malformed or
unsupported declaration rejection. The source-checkout dependency and debug-key
packaging boundaries from the first delivery continue to apply.

A fresh `target/Wave Two Service App` builds the Windows executable and both
Android ABIs with a camera declaration and custom `gpui-box-wave2` scheme. APK
inspection confirms both native libraries and the exact managed service asset.
Doctor reports configured services and the host's missing SDK license record.
The starter installs an explicit activation callback with a visible event count.

The generated APK installed and handled cold `gpui-box-wave2://open/cold` and
warm `gpui-box-wave2://open/warm` Intents through its generated manifest and
managed asset. Native semantics confirmed activation counts 1 then 2. The starter
status uses a semantic Label. A no-change repeat build reports all 36 tasks
UP-TO-DATE. Both Windows and Android builds pass after finalizing the starter.

## Android permission adapter continuation

The Android host now installs the shared native PermissionService before launch.
Generated declarations feed both manifest entries and the host's service asset.
Applications obtain a current `permission_context()` and use the shared service
for asynchronous request and settings operations. Doctor describes the adapter
and keeps configuration separate from current authorization/runtime qualification.
Generated camera, microphone and location features are optional so requesting
permission does not implicitly exclude other device hardware from app stores.
The existing generation regression checks addition and removal of optional camera
hardware along with the matching permission. Stable customization points remain.

See the M1 permission continuation and ADR-014 for runtime evidence. Notification
delivery itself, document/share/export declarations, release signing and updates
retain their own roadmap issues.
