# M3 developer workflow progress

## Command reference and compatibility

`gpui-box reference` renders the current command inventory as English Markdown.
`gpui-box reference --json` returns CommandReference v1 inside the existing
schema-v1 CLI envelope. It includes exact supported syntax, result identities,
file/tool/device effects, offline boundaries, finite/streaming behavior and
stable error codes. Help uses the same typed inventory. Both commands work from
an empty directory and preserve its contents.

The checked-in CLI_REFERENCE.md is generated from this inventory. The ordinary
CLI suite compares it with current output, checks that every syntax also appears
in help, and verifies invalid reference arguments produce deterministic read-only
errors. After editing the inventory, rebuild the CLI and write its reference
stdout as UTF-8 to docs/CLI_REFERENCE.md before running the suite. The command
does not choose a destination or update files itself.

Existing positional commands, template defaults and response fields remain
compatible. Global offline enforcement, watch/add/registry commands, canonical
configuration migrations and provider/platform expansion retain their own
implementation issues. Current project generation, conflict/init recovery,
export integrity and external template qualification are described below.

[Generated command reference](CLI_REFERENCE.md)

## Source-checkout application template

Build `gpui-box-cli` using `mise run build`, then add its `target/debug` directory
to PATH. `gpui-box new` accepts a distribution source checkout or a verified
exported SDK. The generated app uses path dependencies on its facade, components
and official GPUI source, sharing one engine identity. Keep the distribution at
a known revision.

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

## Committed source SDK export

```sh
gpui-box sdk export "/absolute/path/SDK with spaces" --source /path/to/gpui-box
gpui-box sdk verify "/absolute/path/SDK with spaces"
gpui-box new "External Application" --distribution "/absolute/path/SDK with spaces"
```

Export resolves the source checkout root's HEAD once and archives that commit.
The SDK includes the complete committed source tree, preserving the facade,
components, system contracts, upstream GPUI, native host, assets, templates,
license records, root dependency patch and lockfile together. Uncommitted and
untracked source changes are excluded. Committed archive attributes apply.
Global/system attributes and host autocrlf defaults are disabled for export.
Nonempty checkout-local `info/attributes` is rejected because it can override
committed content. Keep exports outside the source checkout's build output.

Root `gpui-box-sdk.json` records schema v1, full source/upstream revisions,
exporter version, scope and a sorted SHA-256 digest for every exported file.
Its content is deterministic for the same commit/exporter and contains no local
source path. Verification checks manifest/provenance compatibility and required
files, rejects changed/missing/unlisted files and linked/special entries, and
enforces portable relative names and content bounds. Export limits are 10000
archive entries, 64 MiB per file and 512 MiB total. Verification counts filesystem
entries including directories toward its 10000-entry bound. Case-colliding,
Windows-reserved, absolute and traversing archive paths are rejected.
`sdk export` and `sdk verify` support finite `--json` responses with
`SDK_EXPORT_FAILED` and `SDK_VERIFICATION_FAILED` errors. Help lists both commands.

The destination parent must exist and be controlled by the caller. Export creates
an exclusive sibling staging directory, verifies it and publishes by rename after
checking that the destination remains absent. Use a stable destination parent
during export. Existing destinations are rejected. Failed staging is retained at
the path printed in the error, with no automatic recursive cleanup. A cleanup
failure after publication explicitly identifies the published SDK. Source-checkout
mode remains available. Treat exported SDKs as immutable inputs. Generation,
including build/dev refresh, verifies an SDK when its root manifest is present.
Keep application and build outputs outside the SDK. Build the CLI from the source
checkout before exporting or use a separately qualified CLI executable.

The manifest provides local content-integrity checks. Its bytes and revision
claims require a trusted source or authenticated distribution channel. Registry
and Git dependency caches, Rust/JDK/NDK/SDK/Gradle toolchains and native CLI
executables are managed separately. Offline builds require those dependencies
to be cached. Authenticated SDK download, content-addressed cache selection,
native CLI publication and unresolved third-party license originals retain #62
and #5 acceptance gates.

### SDK qualification on November 12, 2026

- CLI unit/integration coverage includes 58 Windows cases. New cases exercise
  deterministic manifests across autocrlf settings, WIP/untracked exclusion,
  existing destinations, modified/missing/unlisted files, unsupported schema,
  traversal, incomplete sources, checkout-local attributes and reserved names.
  Unix CI additionally covers links during export and verification.
- Full `mise run check` passes with strict distribution-owned Clippy. The tested
  source SDK revision is `fbdd3f45017662f333c76a523d4b368a0c4702af`, exported into
  a path containing spaces and Chinese characters with 701 committed files.
- A fresh external app in another spaces/Chinese path compiles and links Windows
  desktop output through `gpui-box build desktop` with `CARGO_NET_OFFLINE=true`.
  Desktop interaction is pending because Windows was locked during capture.
- The same app completes Android ARM64/x86_64 construction through the CLI.
  Cargo offline plus Gradle `--offline --rerun-tasks` executes all 36 tasks
  successfully using cached dependencies. An ordinary offline repeat reports
  36 tasks up to date. The SDK still verifies after both platform builds.
- Normal `dev android --device emulator-5554` installs and cold-starts
  `dev.example.gpuibox.sdkqa` on API35 Android15 x86_64, with validated
  ActivityManager completion in 2624ms. Screenshot inspection and a real tap
  confirm Count0 changes to Count1. Physical-device testing remains deferred.

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

## Android device inventory and selection

`gpui-box devices --json` returns schema-v1 inventory data with `completed`,
`platform` and serial-sorted `devices`. Each entry includes `serial`, the raw adb
`state`, `available`, and a deterministic map of adb attributes such as `product`,
`model`, `device` and `transport_id`. Only state `device` is available for app
workflows. Offline, unauthorized, recovery, bootloader and host-permission states
remain visible. Empty inventory is a successful query. Query failures and invalid
adb output produce `DEVICE_QUERY_FAILED`. Human output retains the adb listing,
and JSON mode sends that diagnostic listing to stderr.

Android `install`, `dev` and `logs` resolve one ready device before regeneration,
build or application process lookup. Explicit `--device SERIAL` takes precedence
over `ANDROID_SERIAL`. With neither set, exactly one ready device is required.
An unknown/unavailable requested serial fails even if another device is ready.
Ambiguity includes the ready serials and corrective selection instructions.
Unauthorized/offline/host-permission states receive corrective diagnostics.
The resolved serial is passed to Gradle through `ANDROID_SERIAL` and to adb
through `-s`, keeping installation and launch on the same selected device.
Build-only and packaging operations keep their existing device-independent paths.

The CLI suite passes 38 Windows tests. Four inventory/selection unit cases cover
CRLF, empty/malformed/duplicate output, attribute ordering, multiple/zero ready
devices and unavailable explicit states. A command regression verifies that
missing adb rejects install/dev before managed-file generation or build starts,
including when configuration has pending changes. Real API35 emulator inventory
returns `emulator-5554` with state `device` and native adb attributes. A nonexistent
environment-selected serial is rejected before generation. Physical-device and
non-Windows qualification remain deferred. Explicit `install android --device
emulator-5554 --json` succeeds, followed by the real `dev android --device
emulator-5554` entry point. Gradle installs to that device and adb uses `-s` for
the same serial. The generated application's cold launch returns `Status: ok`
and a live process. Integrated `mise run check` passes 191 host tests plus six
compile-fail doctests and two nested child reports. Broader structured doctor checks,
platform add/list, SDK closure and additional backend qualification remain open
under #61/#62/#73.

## Structured prerequisite diagnostics

`gpui-box doctor --json` returns a schema-v1 DoctorReport with `completed`,
`ready`, `readiness_scope`, host architecture/OS, selected project targets,
project/service schema versions, update channel and an ordered `checks` array.
Every check has a stable `id`, `passed`, `required`, a message and nullable
`corrective_action`. Tool messages include detected versions. Current checks
cover declared assets, mise/Rust/Cargo/Git, Android wrapper pins, pinned native
tools, Rust Android targets, SDK packages/license record and optional release
bundletool/signing environment presence.

Required-check failures set `ready` false and preserve exit status 1 with
`DIAGNOSTICS_FAILED`. Their complete report is available in `error.details`,
while `result` remains null in the existing failure envelope. Successful
diagnostics return the report in `result`. Invalid configuration or inaccessible
distribution fails before a report is produced. Other command errors keep their
existing code/message shape. The optional `details` property is additive.

Optional release prerequisites appear as checks with `required` false and do
not block development readiness. Signing checks report presence only. Keystore
paths, aliases and password values are excluded from both JSON and human
diagnostics. Child diagnostic probes receive closed stdin. Human diagnostics
remain available and JSON mode reserves stdout for the final response.

`readiness_scope` is `declared_prerequisites`. Runtime provider qualification,
desktop linker validation, Android device availability and release credential
validity retain their owning build/runtime/package gates. Service declarations
still produce contextual human guidance, while per-operation capability maturity
remains in each service's evidence matrix. Doctor starts local tool-version
probes and reads wrapper/config/assets/SDK metadata. It preserves project files.

The Windows CLI suite passes 41 tests. New coverage verifies a complete failed
report and corrective actions with missing tools/assets/SDK, original-file
preservation, signing-value exclusion and optional/required readiness aggregation.
On this host, the existing Android/desktop generated app returns a failed report
whose only required failure is the missing Android SDK license record. Optional
release inputs are also absent. A separate desktop-only generated application
returns a successful report with the detected mise/Rust/Cargo/Git versions.
Integrated `mise run check` passes 194 host tests plus six compile-fail doctests
and two nested child reports. Physical-device testing remains deferred. SDK
closure, canonical schema/migrations, platform add/list and broader host/backend
qualification retain the open #61/#62/#73/M3 gates.

## Starter initialization and target selection

The version-1 starter supports `new PATH --distribution PATH` and `init PATH
--distribution PATH`. Both accept `--targets desktop`, `--targets android` or
`--targets desktop,android`. The default remains both targets. Options accept one
value each and repeated/unknown options fail before project creation. Only the
selected platform manifests, workspace members, source entry points, native host
and mise target tasks are created. Shared source/components/assets retain one
GPUI identity and the existing fixed upstream patch. Android uses minimum and
target API35 with compile API36. `.gpui-box/template.json` records starter ID,
template/schema versions, CLI version, upstream revision and selected targets.
Existing applications remain valid without this additive provenance file.

`init` requires an existing directory. It builds a complete sibling staging
project, uses the destination directory name as the application name and performs
ownership preflight under an exclusive `.gpui-box-init.lock`. Existing README.md,
LICENSE and .gitignore files are preserved byte-for-byte. Other colliding
template files, linked output paths and active init locks cause failure before
publication. Unrelated files and existing directories are preserved. Integrate
the distribution's template guidance into retained .gitignore and README files
as needed, preserving their application-specific content.

Each new file is published through an exclusive hard link, which requires
hard-link support on the project filesystem. `gpui-box.json` is published last
as the completion marker. Publication can stop partway on filesystem failure or
interruption. The complete sibling staging tree is retained for inspection and
recovery. Published files share storage with that staging copy until its link is
removed. Verify no initializer remains running before removing a stale init lock.
For an interrupted init, compare the published files with staging and complete
the missing copies while preserving application edits, then use `generate`.
An ordinary retry rejects conflicting files. Successful publication removes only
the exact generated staging entries. It uses no recursive caller-tree deletion.

Repeated init reports an already-initialized project and directs the caller to
`generate`. Configuration changes continue through generation plans/journals.
Changing the selected targets later updates managed manifests. Disabled-target
files and their ownership hashes are retained, allowing conflict-checked
re-enablement. Old user-owned source/native customization files retain the
existing preservation contract. Adding a target that the starter never created
requires its source entry point and mise/native customization setup. Automated
platform-add provisioning retains #73's gate.
Existing Cargo workspaces require explicit application integration when their
Cargo.toml conflicts. Workbench/component templates, authenticated SDK
distribution and canonical schema migrations retain #63/#62/#61 acceptance gates.

Windows CLI qualification passes 48 tests and strict Clippy. Added regressions
cover single-target output/provenance, no-op generation, init with existing
documents, repeated init, ownership conflicts, active locks and invalid options.
Linux CI also runs linked-output rejection. External applications in paths with
spaces and Chinese characters complete a Windows desktop compile/link and an
Android ARM64/x86_64 debug APK build through the normal CLI. The Android build
executes all 36 Gradle tasks. AGP's default Windows non-ASCII path rejection was
encountered before compilation. The managed host now sets
`android.overridePathCheck=true`, and that complete build qualifies the tested
JDK17/AGP8.10.1/NDK30/mise tuple. Other tool versions retain their own validation.

## Adaptive application template

```sh
gpui-box new "Adaptive Application" --distribution /path/to/sdk --template adaptive
gpui-box init /path/to/existing/app --distribution /path/to/sdk --template adaptive --targets android
```

`new` and `init` accept `--template starter|adaptive`. Starter remains the default
with both desktop and Android targets. The adaptive template reuses the same
workspace/native generation, SDK verification, target selection, font assets and
init ownership protections. Its JSON result and `.gpui-box/template.json` record
the selected template ID and version1. Unknown, missing or repeated template
options fail before initialization. Regeneration preserves the application-owned
template source and provenance. Choosing another template is a new/init decision.

The shared example uses the delivered application bar and master-detail
components with a 720 logical-pixel breakpoint. Compact layout opens a detail
page. Wide layout presents both panes. A retained selection, text-input entity
and explicitly committed draft survive layout changes and navigation. The draft
is shared across items. Back to items, Escape and Android system back return from
compact detail and restore the selected master focus. Tab/Shift+Tab follow the
shared focus traversal. Keyboard insets, safe areas and vertical overflow are
handled by the composed view. Custom URL activation keeps the starter callback.
Business persistence and process-death restoration use the documented application
service/snapshot hooks and remain application-owned.

Qualification uses a fresh external spaces/Chinese app path and the committed
SDK from the export evidence above. Windows desktop build/link and strict app
Clippy pass with cached dependencies. Android ARM64/x86_64 builds and normal
CLI install/cold launch pass on API35 Android15 x86_64. Final launch completion
is2975ms. Real taps select Shared components, enter and commit FinalDraft42.
Portrait-to-landscape-to-portrait recreates native surfaces and preserves the
selection, draft and committed text. Native UI hierarchy assertions confirm Wide
and Compact layouts plus retained EditText value. System back restores the
selected master. Emulator rotation policy is restored to its original free mode.

CLI60Windows unit/integration cases pass, including all target combinations,
template provenance, source preservation, existing-directory init and repeated
template option rejection. Full `mise run check` passes after integrating the
collaborator's #98 menu contracts. Desktop interaction remains pending while
Windows is locked. Workbench/component templates, broader assistive-reader and
host qualification remain #63/#3 acceptance gates. Physical-device testing
remains deferred.

## Component author template

```sh
gpui-box new "Component Preview" --distribution /path/to/sdk --template component
gpui-box init /path/to/existing/preview --distribution /path/to/sdk --template component --targets desktop
```

The component-v1 template uses the existing new/init staging, target selection,
provenance and ownership contract. Its application-owned specimen composes the
delivered input, checkbox, switch, button, label and application-header controls.
Replace the specimen in app/src/lib.rs and keep common editing/focus/semantic
behavior in the coordinated component crate. The template supports desktop,
Android and combined target sets. Default new/init behavior remains starter.

Preview settings include dark/light palettes, high contrast, reduced motion and
1.25x text scale. Theme changes update the retained input entity. Checked state,
text and activation count remain in the shared view during resizing and Android
Activity recreation. Compact/wide arrangement changes at720logical pixels with
vertical scrolling, safe-area padding and keyboard avoidance. Tab/Shift+Tab
traverse controls, Enter/Space activate them and Escape blurs/hides the keyboard.
Settings actions retain their current control focus for keyboard continuation.
The motion readout exposes the selected duration policy. Custom animations can
consume that policy when added by the component author.

Labels, checked states and editable values are available through the delivered
platform semantic adapter. Screen-reader speech, process-death persistence,
registry/publication metadata, isolated arbitrary-item preview commands and
workbench/module generation retain their own acceptance gates.

A fresh external Windows spaces/Chinese path completes desktop compilation/link
in43.09s and strict application Clippy. Android builds both native ABIs through
the normal CLI with all36Gradle tasks executed in2m28s. Normal dev installs and
cold-starts on API35 Android15 x86_64 in2180ms. Actual taps toggle all settings,
check the checkbox and increment the specimen. UiAutomation verifies native
checked states, reduced-motion duration0ms and PreviewDraft42 editor text.
Portrait/landscape/portrait retains all settings, draft and activation count.
Tab/Space toggles the checkbox, then Tab/Enter activates the specimen. Rotation
policy is restored to its original free state. Initial screenshot inspection
found the editor's default light theme in the dark preview. The template now
sets the retained editor's initial theme explicitly before its first frame.

## Reproducible external template qualification

`mise run check:templates` runs the opt-in real-tool qualification test. Set
`GPUI_BOX_QUALIFICATION_TARGET` to `desktop` or `android` and optionally set
`GPUI_BOX_QUALIFICATION_REPORT_DIR` to a fresh absolute report directory. The
default is `target/ci/template-qualification`. Existing reports are preserved
by rejecting reuse. Install and trust the repository's pinned mise environment
and the selected host's native build prerequisites first. Android additionally
requires the documented SDK/NDK packages and `ANDROID_HOME`.

```powershell
$env:GPUI_BOX_QUALIFICATION_TARGET = 'desktop'
$env:GPUI_BOX_QUALIFICATION_REPORT_DIR = 'D:\qualification-reports\run-1'
mise run check:templates
```

```sh
GPUI_BOX_QUALIFICATION_TARGET=android \
GPUI_BOX_QUALIFICATION_REPORT_DIR="$PWD/target/ci/template-qualification" \
mise -E android run check:templates
```

The harness exports the checkout's committed HEAD through the actual CLI and
verifies the complete SDK before and after qualification. It creates nine
applications from starter/adaptive/component-v1 crossed with desktop, Android and combined
target sets. The fresh external workspace, SDK and application paths include
spaces and Chinese characters. Each application receives an application-source
customization and, when enabled, a native Gradle customization. No-op generation,
unchanged template provenance and preserved customizations are asserted. A build
plan must reject the selected host target when disabled. Six enabled cases
run the normal `gpui-box build TARGET` entry point and produce a nonempty desktop
executable or APK containing both ARM64/x86_64 native libraries.

Only freshly generated mise configurations from this checkout's templates are
trusted by the harness. The harness requests an isolated Cargo workspace output
directory through child environment and resolves desktop artifacts through Cargo
metadata. Cargo wrappers/configuration retain their output-layout policy.
Dependencies and tool downloads follow normal
Cargo/mise/Gradle cache and network configuration. The SDK export uses committed
content, so commit template/SDK changes before qualifying that source revision.
The SDK includes its available license originals and provenance inventory.
Outstanding license-source review remains documented under tooling/licenses.

The report contains CLI/SDK/upstream identities, host/target, template provenance,
commands with separated argument boundaries, exit codes, durations, artifact
SHA-256 values and explicit `runtime_executed: false`. Each child has separate
stdout/stderr logs. The receipt is written on a qualification error as well.
Logs inherit the tools' normal diagnostic behavior and should use development
credentials only. The workspace and all application/SDK content remain available
at the path in receipt.json for inspection. Clean that exact workspace after
inspection when no child process is using it. The harness performs no recursive
caller-tree cleanup.

Linux and Windows hosted jobs select desktop. The Android hosted job selects
Android. Their existing validation artifacts retain the reports for14days.
These checks qualify generated builds and customization preservation. Interactive
desktop/emulator sessions, signing/install/update journeys, offline dependency
provisioning, registry combinations, native CLI publication, workbench
templates and physical-device acceptance retain their operation-specific gates.

## Read-only build plans

Use `gpui-box build desktop --dry-run --json` or `gpui-box build android
--dry-run --json` for a schema-v1 BuildPlan. The plan includes operation,
host architecture/OS, configured target, toolchain target labels, the complete
generation ChangePlan, and the command's program, argument vector, working
directory, inherited-environment flag and explicit environment overrides.
Android currently builds both ARM64 and x86_64 at API35 through the pinned
Gradle host. Desktop uses the host's default Rust target. Application-owned
Gradle customization and inherited Cargo/mise configuration retain control of
their downstream tool behavior.

Preview reads local configuration/distribution inputs and managed ownership
metadata. It creates no directories/locks/journals/output and starts no tool
process or network request. It works before managed outputs have been generated
and when local build tools are absent. Conflicts, active generation locks,
invalid configuration and disabled targets produce `BUILD_PLAN_FAILED`.
Human preview prints the command summary. JSON contains the full typed plan.

The real build entry point and preview share command construction. Actual Android
execution still regenerates files and verifies wrapper pins before starting
Java. Actual builds recompute current inputs. A preview is a snapshot and does
not reserve state for later execution. Build plans list subsequent generation,
child lockfile/build/cache writes and potential dependency downloads separately.
Inherited environment values are excluded from the plan. Build dry-run applies
to the default build command with no device override. Install/dev device selection
retains its separate adb workflow.

## Android launch result validation

`dev android` captures and drains both adb launch streams with a 512 KiB bound
per stream and a 60-second local adb wait deadline. Launch succeeds only when
the child exits successfully and ActivityManager reports exactly one
`Status: ok`, the requested `Activity:` identity and `Complete`. Explicit errors,
Java exceptions, timeouts, ambiguous/missing statuses and different activities
fail the workflow even if adb exits zero. Diagnostics remain available in human
output. The deadline terminates and reaps this invocation's adb client. Android
may have received the launch request before failure, so a failed report still
requires checking current device state before retrying. Other adb clients and
the shared server keep their existing lifecycles.

Regression coverage includes cold-success/CRLF, missing completion, wrong target,
timeouts/errors/duplicate statuses, bounded output and actual subprocess success,
stderr-error and nonzero exit. The normal generated-app `dev android` path
installs and cold-starts on API35 x86_64 in 1446ms with validated output.
This confirms ActivityManager launch completion. Later rendering, crash and
interactive acceptance retain their separate gates. Physical devices remain
deferred. Watch builds and process-group development supervision stay under #72.

The Windows CLI suite passes 43 tests. Two new command groups cover deterministic
plans with pending generation changes, explicit argument boundaries in
space-containing paths, unavailable tools/SDK, secret exclusion, uninitialized
projects and read-only failures. Integrated `mise run check` passes 196 host
tests plus six compile-fail doctests and two nested child reports. A real generated
application previews both targets without rewriting its manifest, then completes
Android and Windows desktop builds through the normal CLI entry points.
Physical-device testing remains deferred. SDK closure, explicit Rust target
selection, broader platform add/list and the remaining #61/#62/#73 gates stay open.
