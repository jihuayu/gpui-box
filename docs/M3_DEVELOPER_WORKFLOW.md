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
and update channels are described in the [M4 packaging workflow](M4_ANDROID_PACKAGING.md)
and M5. `package android --release` now builds signed/inspected APK and AAB.
`package desktop --portable` builds a Windows developer ZIP. Desktop installers
remain in M7.

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

## Machine-readable CLI response v1

Add `--json` once, before or after the existing positional arguments, for a
single newline-terminated JSON value on stdout. The finite commands `info`,
`--version`, `help`, `new`, `generate`, `doctor`, `devices`, `build`, `install`
and `package` support this response envelope. Existing human output remains the
default. All progress, diagnostics and child-process stdout use stderr in JSON
mode. Child build/install processes receive closed stdin. The command exit status
remains zero on success and nonzero on failure.

```json
{
  "schema_version": 1,
  "cli_version": "0.1.0-dev",
  "upstream_revision": "f8cfc7ed9e57503def9479138894f3139f5400cc",
  "command": "generate",
  "success": true,
  "result": { "completed": true },
  "error": null
}
```

On failure, result is null and error contains stable `code` and explanatory
`message` fields. Consumers branch on code and response schema. Diagnostic text,
OS messages, paths and child output are human-facing. Those strings can change
and may include local project information. `info` returns structured version,
upstream, host and target-family fields. `new` returns the destination. Help
returns its text and response-schema version. Other commands return completion
status. Per-check doctor data, parsed device inventories, artifact result paths
and streaming progress events remain future schema additions.

| Error code | Meaning |
| --- | --- |
| INVALID_ARGUMENTS | Unknown command, invalid shape or repeated --json |
| CURRENT_DIRECTORY_UNAVAILABLE | Current working directory cannot be resolved |
| DISTRIBUTION_UNAVAILABLE | New project's distribution path cannot be resolved |
| PROJECT_CREATION_FAILED | App creation failed, including destination conflicts |
| GENERATION_FAILED | Configuration, managed-file conflict or generation failed |
| GENERATION_PLAN_FAILED | Read-only generation snapshot or preflight failed |
| DIAGNOSTICS_FAILED | Doctor configuration or prerequisite checks failed |
| DEVICE_QUERY_FAILED | SDK/adb discovery or device enumeration failed |
| WORKFLOW_FAILED | Build/install/debug-package workflow failed |
| PACKAGING_FAILED | Inspected release or portable packaging failed |
| STREAMING_JSON_UNSUPPORTED | dev/logs requested the finite-response format |

`dev` and `logs` use streaming human output. JSON requests for these commands
fail before project mutations or tool startup. Duplicate `--json` also fails
deterministically. Project schema remains the delivered `gpui-box.json` v1 with
service schema v1. This slice adds no interactive prompts. Canonical schema
migrations/precedence, dry-run/offline guarantees and streaming event contracts
remain tracked by #61 and related CLI tasks.

The CLI suite passes 22 tests. Five new command-level cases cover structured
info/help/version, invalid/duplicate arguments, finite/streaming errors, and new
and regenerate in a path containing spaces and Chinese text while preserving
application-owned source and invalid configuration files.

Integrated `mise run check` passes 175 host tests plus six compile-fail doctests
and two nested child-process reports. Real `devices --json` keeps adb's emulator
inventory on stderr and returns a parseable success envelope. The existing
space-containing Wave Two Service App completes `build android --json` with
35/36 Gradle tasks up to date and one bookkeeping task executed. The actual
Gradle/native output stays on stderr. `doctor --json` returns DIAGNOSTICS_FAILED
with exit code 1 for this host's missing Android SDK license record, retaining
the corrective diagnostic on stderr. No physical device was used.

## Read-only generation plans

Run `gpui-box generate --dry-run` for a human summary, or add `--json` for a
schema-v1 ChangePlan in the response's result. Planning uses the same generated
bytes, managed-file ownership checks and pending-journal recovery preflight as
ordinary generation. It reads local configuration/distribution files and starts
no build tool or network request. It creates no metadata directories, locks,
journals or temporary files, including on failures and uninitialized projects.

The plan records CLI/upstream revision, the exact project-config SHA-256, pending
recovery status, path-sorted managed files and the generated-state metadata file.
Each FileChange contains `path`, `action` (`create`, `update`, `unchanged`),
nullable `before_sha256` and required `after_sha256`. Paths use project-relative
forward slashes. The state file is listed separately as `metadata`. Application
source/native customizations remain outside managed outputs. Transient journal
and lock writes are lifecycle operations for actual generation.

An existing generator lock makes planning fail. A second lock check detects a
generator that started while planning. The result is a read-only snapshot and
has no filesystem transaction isolation. Ordinary `generate` recomputes current
inputs and preconditions under its existing lock. Editing a managed file after a
preview still fails before managed-file writes. Saved-plan replay is described
below. Isolated multi-file transactions remain a #65 follow-up.

Both planning and generation reject symbolic links and Windows reparse points
in existing managed-output/metadata path components. Project roots remain
application-owned while tools run. This validation retains that trusted-directory
boundary. Unknown files remain untouched. Corrupt state/journals, edited managed
files and invalid configuration fail with their originals preserved. A pending
journal is reported by planning and completed only by actual generation.

The Windows CLI suite passes 27 tests, with five new plan regressions covering
determinism, exact hashes and apply correspondence, absent metadata, failure
preservation and pending recovery. Read-only tests compare directory/file names,
file digests and file modification times. Delayed Windows directory timestamps
are excluded from those assertions. A real space-containing generated project
reports 15 unchanged managed files and preserves its file hashes/timestamps.
An actual Windows junction in a managed path is rejected. A Unix symlink
regression is included and awaits a Unix runner.

## Saved generation plans and apply receipts

`gpui-box generate --plan PATH [--json]` replays the bare ChangePlan from a
preview's `result`. Save that object as UTF-8 JSON. In PowerShell, for example

```powershell
$preview = gpui-box generate --dry-run --json | ConvertFrom-Json
$planJson = $preview.result | ConvertTo-Json -Depth 20
[IO.File]::WriteAllText((Join-Path $PWD "generation-plan.json"), $planJson,
    [Text.UTF8Encoding]::new($false))
gpui-box generate --plan generation-plan.json --json
```

The parser accepts at most 1 MiB and rejects unknown fields, malformed JSON and
the enclosing CLI response. Plans supply preconditions. Output bytes are rendered
again from the current distribution. Apply acquires the existing project lock,
recomputes the plan and requires exact equality, including CLI/upstream identity,
configuration bytes, managed-file hashes, metadata and pending-journal state.
The additive `pending_journal_sha256` field binds the exact existing journal
bytes, or is null when no recovery is pending. Application-owned source and
native customization changes remain outside those preconditions.

Generation rechecks current configuration, outputs, metadata and journal before
applying. It checks each managed output again before replacement, checks final
output hashes before committing generated state, and checks its journal before
removal. A detected edit stops the operation and retains the recovery journal.
Individual writes use synced sibling files and rename. A failure can leave a
prefix of outputs updated. `generate` resumes using the retained journal, while
an old saved plan requires a fresh preview because its preconditions changed.
Project-directory ownership during execution remains required. External edits
between a check and rename, parent-directory durability and all-or-nothing
multi-file isolation retain their existing qualification boundaries. Build and
package commands have separate side effects and keep their existing contracts.

Both ordinary generation and saved-plan replay return a schema-v1 ApplyReceipt
in JSON mode. It includes `completed`, `plan_sha256`, ordered `files_written`
and `files_unchanged`, and `recovered_pending_journal`. The lists cover managed
outputs followed by generated-state metadata. Transient locks and journals are
excluded. `plan_sha256` hashes the CLI's compact UTF-8 serialization of the
validated ChangePlan in its declared field order. It is stable for the same
plan and independent of whitespace/key ordering in the saved file. A failed
apply returns `GENERATION_FAILED` and exit status 1. Receipts are emitted only
after completion. Human commands retain their existing messages.

Unchanged managed files and metadata retain their bytes and modification times.
A fully unchanged generation skips the journal and output writes while still
using the operation lock. Recovery can remove a pending journal without
rewriting already-correct outputs.

The Windows CLI suite passes 33 tests. Six replay regressions cover exact
write/hash correspondence, deterministic no-op receipts and timestamps,
application-owned edits, stale configuration/output/metadata/journal/lock state,
bounded strict parsing, and Windows file-lock interruption followed by ordinary
generation recovery. That interruption test denies replacement of a later
managed file, verifies the earlier writes and original state/journal, rejects
the old plan, then completes recovery after releasing the handle. Physical-device
testing remains deferred. Registry installation, archive extraction, broader
SDK/offline/migration policy and non-Windows qualification remain tracked by
#61/#65 and their related issues.
