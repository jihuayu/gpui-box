# Structured diagnostics, crash records and recoverable export

This document specifies `crates/gpui-diagnostics` for
[#68](https://github.com/jihuayu/gpui-box/issues/68). The crate builds on the
`gpui-system` contracts from [#19](https://github.com/jihuayu/gpui-box/issues/19),
the private atomic storage in `gpui-app-data`, and the pinned export leases in
`gpui-files` ([#23](https://github.com/jihuayu/gpui-box/issues/23)). It installs
no native crash collector and never claims a platform qualification it has not
earned. Native signal/SEH/MetricKit/JNI collectors remain explicit host work.

## Scope and ownership

`gpui-diagnostics` owns:

- Structured event vocabulary, correlation identity and bounded event storage.
- Deterministic sensitive-data redaction at construction and ingestion.
- Atomic `AppData` persistence under the dedicated `diagnostics` namespace.
- Bounded event/crash retention with cumulative eviction accounting.
- Session markers, crash markers and relaunch recovery.
- Corrupt-record quarantine that preserves raw forensic files.
- Capability-gated `DiagnosticsService` records, recovery, pruning and export.
- A versioned JSON export staged through `gpui_files::ExportResourceLease`.
- A six-target per-operation configuration and qualification matrix.

It does **not** own:

- Native signal handlers, Windows SEH, Android JNI/ANR/tombstone APIs, iOS
  MetricKit, WebWorker storage or browser download UI.
- The `AppData` root policy. Android and iOS hosts must inject a private
  container root; no desktop path is inferred on mobile.
- Renderer/GPU trace collection. Those collectors remain backend work.
- Crash-report upload, symbolization services or remote telemetry consent.

`gpui-box` exposes the crate through opt-in features:

```toml
gpui-box = { features = ["diagnostics"] }
gpui-box = { features = ["diagnostics-gpui"] }
```

`diagnostics` imports `gpui_box::diagnostics` and enables the system, app-data
and files dependencies. `diagnostics-gpui` additionally compiles the optional
GPUI adapter and the shared system/files GPUI integrations. Neither feature
selects a desktop or Android backend by itself.

## Architecture

```mermaid
flowchart LR
    Host[Host entrypoint / UI] --> Service[DiagnosticsService]
    Service --> Gate[Capability + context gate]
    Gate --> Provider[DiagnosticsProvider]
    Provider --> InProcess[InProcessDiagnosticsProvider]
    Provider --> Fake[FakeDiagnosticsProvider]
    InProcess --> Store[DiagnosticStore]
    Store --> Redact[Deterministic redactor]
    Store --> AppData[AppData diagnostics namespace]
    Store --> Ring[Bounded events + crash records]
    Store --> Session[Session / crash markers]
    Store --> Quarantine[Corrupt-record quarantine]
    Service --> Export[ExportResourceLease]
    Export --> Files[gpui-files staging]
    Files --> AppData
    Service --> Stream[Bounded DiagnosticServiceEvent stream]
    Adapter[Optional GPUI adapter] --> Service
    Adapter --> System[SystemServices / App global]
```

`DiagnosticsService` is a clonable `Send + Sync + 'static` handle backed by
`Arc<dyn DiagnosticsProvider>`. It can be registered in
`gpui_system::SystemServices`, installed as a GPUI `App` global, or created
directly with `DiagnosticsService::in_process(&app, build_identity)`.
`InProcessDiagnosticsProvider` wraps a `DiagnosticStore`; the deterministic
`FakeDiagnosticsProvider` supports capability overrides, unavailable-collector
markers, failures and simulated disk-full conditions.

### Execution and context contract

Every blocking operation (`recover`, `record_event`, `write_crash_marker`,
`inspect_recovery`, `export_bundle`, `prune_retained`, `mark_clean_exit`) requires
a background `OperationContext`:

1. `context.ui.is_some()` returns `Err(SystemError)` with
   `ErrorCode::InvalidInput` **before any side effect**.
2. An elapsed deadline returns `ErrorCode::TimedOut`.
3. A cancelled context returns `Ok(Completion::Cancelled(reason))` before any
   side effect.
4. The service then checks the operation's `Capability<DiagnosticsOperation>`
   before touching the provider. Unsupported/unknown support, missing
   configuration, unavailable providers and denied permissions are typed errors.
5. Cancellation is rechecked before commits and during export. A cancellation
   after a durable commit cannot undo that commit, but it is never reported as a
   newly staged export.

`DiagnosticStore::open` only computes the namespace path and retains the build
identity; it performs no writes. The first successful `recover` (or the first
operation that lazily calls it) reads persisted state, applies recovery, and
writes the new active `SessionMarker`. `DiagnosticsService::recover` is the
capability-gated entrypoint for hosts.

## Correlation domains

`DiagnosticDomain` has six values:

| Domain | Typical emitter | Correlated work |
| --- | --- | --- |
| `Command` | Command dispatcher / menu action | Command ID, request ID |
| `NativeRequest` | JNI, C ABI, Swift or platform callback | Request ID, surface, generation |
| `HostGeneration` | Window/surface creation or recreation | Surface, generation |
| `UiCommit` | UI scene/commit scheduler | Scene/commit revision, surface |
| `Renderer` | Renderer frame/trace boundary | Revision, surface, job ID |
| `Background` | Worker executor / background job | Job ID, optional request ID |

`CorrelationContext` carries optional `command_id`, `request_id`, `surface`
(a `SurfaceId`), `surface_generation`, `revision` (the scene/commit revision) and
`job_id`. All are optional; an event with no correlation is still valid. Builders
(`with_command_id`, `with_request_id`, `with_surface`,
`with_surface_generation`, `with_revision`, `with_job_id`) keep construction
readable. Correlation values are numeric/opaque and never contain paths or
credentials.

## Strict sensitive-data redaction

Redaction runs in `DiagnosticField::new`, `RedactedValue::text/list/object`,
`DiagnosticEvent` construction/deserialization, every crash/termination evidence
constructor and every deserialization path. Raw strings are not retained in a
public diagnostic type. Each redacted value is replaced by a stable marker:

```text
[REDACTED:<kind>]
```

Kinds are `token`, `secret`, `password`, `authorization`, `cookie`,
`credential`, `private_key`, `api_key`, `session`, `path`, `bearer`, `basic`,
`jwt`, `url_userinfo` and `url_query`.

### Sensitive keys

Field keys are normalized to lowercase underscore-separated tokens, including
camel-case boundaries and punctuation (`apiKey` → `api_key`,
`x-api-key` → `x_api_key`). A key containing any of these normalized
tokens/substrings replaces its **entire value**, including nested objects,
lists and numbers with `[REDACTED:<kind>]`:

| Kind | Matching key signals |
| --- | --- |
| `private_key` | `private_key`, `privateKey`, `private...key` |
| `api_key` | `api_key`, `apiKey`, `x-api-key` |
| `authorization` | `authorization`, exact/terminal `auth` |
| `cookie` | `cookie` |
| `credential` | `credential`, `credentials` |
| `password` | `password`, `passwd` |
| `token` | `token`, `access_token`, `id_token` |
| `secret` | `secret`, `client_secret` |
| `session` | `session`, `session_id` |
| `path` | `path`, `file_path`, `filepath`, `request-path` |

Matching is deliberately conservative: over-redacting a diagnostic key is
preferred to leaking a credential or location.

### Sensitive string values

For non-sensitive keys, free-form strings and every nested/list/object value pass
through the same deterministic pipeline:

- Authorization schemes: `Bearer ...` → `[REDACTED:bearer]`; `Basic ...` →
  `[REDACTED:basic]`.
- JWTs beginning with `eyJ` and containing at least three dot-separated
  segments → `[REDACTED:jwt]`.
- URLs with userinfo (`https://user:pass@example.com/...`) →
  `[REDACTED:url_userinfo]` (the whole URL token is replaced).
- URLs with sensitive query parameters (`?token=`, `&api_key=`,
  `&access_token=`, `&secret=`, `&password=`, `&session=`, `&credential=`, and
  the other key classes above) → `[REDACTED:url_query]`.
- Sensitive `key=value` assignments in free text (`token=...`, `password=...`)
  replace the value with the key's redaction kind.
- Unix absolute paths: `/home/...`, `/Users/...`, `/var/...`, `/tmp/...`,
  `/etc/...`, `/opt/...`, `/private/...`, `/mnt/...`, `/proc/...`, `/root/...`
  and general multi-segment `/a/b` paths → `[REDACTED:path]`.
- Windows absolute and UNC paths: `C:\...`, `D:/...`, `\\server\share\...`,
  `\\?\C:\...` → `[REDACTED:path]`.
- Harmless text (`and/or`, dates, versioned relative documentation paths,
  ordinary URLs without userinfo/sensitive queries) is preserved.

Redaction is deterministic and idempotent: re-running it cannot turn a marker
into another secret-looking value.

### Debug and serialization guarantees

`RedactedValue`, `DiagnosticField`, `DiagnosticEvent`, `CrashRecord`,
`TerminationKind` and every evidence payload implement `Debug` summaries that
never print a raw payload. `RedactedText` is an opaque newtype whose only public
constructor redacts; direct raw variant construction is not possible. Serialized
events and crash records contain only sanitized values. Tests assert zero
canary leakage through both `Debug` and JSON for events, crash evidence,
quarantine reports and the full export bundle.

## Atomic `AppData` persistence

The store uses `app.location(DataLocation::Data).child("diagnostics")`:

| File | Purpose |
| --- | --- |
| `events.json` | Versioned retained event envelope and `evicted_events` counter. |
| `crashes.json` | Versioned retained crash-record envelope and `evicted_crash_records` counter. |
| `session.json` | Active/clean `SessionMarker` for the current session. |
| `crash-marker.json` | Host-written RustPanic/AndroidAnr/NativeCrash/OsTermination marker. |
| `quarantine/` | Intact corrupt/truncated/unparseable forensic files. |

All writes stage a same-directory `tempfile`, write and `sync_all`, then
atomically persist/rename. Envelopes carry an explicit format and schema
version. `deny_unknown_fields` and validation reject malformed or unsupported
envelopes. An interrupted staging write leaves the previous committed file
untouched; staging files are never treated as committed records.

`StorageHook` exposes `before_write` and `before_rename` hooks.
`StorageFaultInjection` provides one-shot `fail_next_write` and
`fail_next_rename` failures (`io::ErrorKind::StorageFull`). Tests verify that a
write or rename failure during append/recovery leaves previously committed
records byte-for-byte intact and readable. A committed write never reports a
misleading pre-commit error. Filesystem crash consistency ultimately depends on
the host filesystem and power-loss policy; same-directory rename is the
strongest portable guarantee available here.

`cleanup_staging_files` removes only abandoned `.diagnostics-*` regular files;
it never touches committed records or quarantine entries.

## Bounded retention and eviction accounting

`RetentionPolicy` has three configurable bounds:

| Field | Meaning |
| --- | --- |
| `max_events` | Maximum retained events. |
| `max_byte_budget` | Serialized byte budget for retained events. |
| `max_crash_records` | Maximum retained crash records. |

On append and on explicit prune:

1. Oldest entries are removed first, deterministically.
2. Every removal increments the persisted cumulative `evicted_events` (or
   `evicted_crash_records`) counter.
3. The byte budget is calculated from each retained event's serialized JSON
   size.
4. If a single event exceeds `max_byte_budget`, existing events are evicted
   deterministically and the oversized event itself is evicted rather than
   exceeding the budget or corrupting the log. The append succeeds with
   `RecordOutcome::stored == false` and the eviction is counted.
5. `max_events = 0` or `max_crash_records = 0` means no retained entries, not a
   panic.

`RecoveryReport` exposes both current counts and cumulative evictions;
`PruneReport` reports the retained counts after an explicit prune. Counters
survive process restart because they are part of the versioned envelopes.

## Session, crash and relaunch recovery

A `SessionMarker` contains the session ID, build identity, start time and
`clean`. `mark_clean_exit` sets `clean = true` and best-effort removes any stale
crash marker. On relaunch, `recover` reads the prior session and crash marker
before writing the new session:

| Prior state | Recovery result |
| --- | --- |
| Prior marker `clean = true` | Report `CleanExit`; create no crash record. |
| Active marker + matching crash marker | Adopt the marker into a retained `CrashRecord`, preserving the exact `TerminationKind` and sanitized evidence; delete the marker. |
| Active marker, no crash marker | Synthesize `UncleanShutdown` referencing the prior session ID and build identity. |
| No prior marker + orphan crash marker | Adopt as an orphan `CrashRecord` rather than silently losing evidence. |
| Corrupt marker/session | Quarantine the raw file and continue without panicking. |

Duplicate adoption is avoided by matching session ID plus marker timestamp (and
by matching an existing `UncleanShutdown` record for the same prior session).
This makes recovery retryable after a partial write failure.

`TerminationKind` variants and evidence:

| Variant | Evidence | Source |
| --- | --- | --- |
| `CleanExit` | None. | `mark_clean_exit` / recovery summary. |
| `RustPanic` | Redacted message, optional location, stack frames. | Panic hook via `write_crash_marker`. |
| `AndroidAnr` | Redacted reason, frames, optional tombstone text. | JNI ANR/tombstone bridge. |
| `NativeCrash` | Optional signal, exception type, frames, detail. | SEH/signal/Mach/native crash reporter. |
| `OsTermination` | Redacted reason, exit code, exit reasons. | OS exit-reason bridge. |
| `UncleanShutdown` | Prior session ID and build identity. | Synthesized by recovery. |

Only the four external crash evidence kinds can be written through
`write_crash_marker`; `CleanExit` and `UncleanShutdown` are lifecycle
conclusions and return `ErrorCode::InvalidInput`.

### Corrupt-record quarantine

Each persisted file is read with `symlink_metadata`, a 64 MiB bound and a
format/schema/termination validation. A file is quarantined when it is:

- invalid UTF-8/JSON or truncated;
- an unknown/unsupported format or schema version;
- a symlink, directory or oversized record;
- a crash marker with a lifecycle-only termination kind.

Quarantine creates `diagnostics/quarantine/`, chooses a unique
`<original>.<timestamp>[.n]` name, and renames the **raw file** intact. Nothing
is deleted. `RecoveryReport.quarantined_entries` records the original name,
stored name and byte size (never an absolute path), and
`RecoveryReport.quarantined_count` is kept in sync. Tests corrupt all four record
files and verify each forensic file still contains the original corrupt bytes.

## Recoverable export bundle

`DiagnosticsService::export_bundle(&AppData, &OperationContext)`:

1. Rejects UI contexts and invalid deadlines/cancellation.
2. Checks `Capability<DiagnosticsOperation::ExportBundle>` before any side
   effect.
3. Builds the bundle, rechecking cancellation before serialization and again
   before staging.
4. Serializes the versioned JSON `DiagnosticExportBundle`.
5. Stages it through
   `gpui_files::ExportResourceLease::stage_export_bytes(app_data, ...)`.
6. Returns the lease; the inner `gpui_app_data::ExportLease` pins the file
   against `cleanup_exports(Duration::ZERO, ...)` until `release()`/drop.

Cancellation before or during export returns `Completion::Cancelled` and leaves
no active/staged export file. A cancellation after the lease is committed is not
reported as a new staging operation. `Debug` for the lease never prints the
export path.

### Bundle schema

| Field | Contents |
| --- | --- |
| `format` | `"gpui-box.diagnostics"`. |
| `format_version` / `schema_version` | `1` / `1`. |
| `generated_at_millis` | Export time. |
| `build_identity` | Package version (workspace `0.1.0-dev`), upstream GPUI revision `f8cfc7ed9e57503def9479138894f3139f5400cc`, target triple, profile, channel, build/symbol IDs and optional source/toolchain/OS metadata. |
| `session_id` | Current diagnostics session. |
| `platform` | `PlatformTarget`, `os`, `arch`, `family`. |
| `capabilities` | Summary for every `DiagnosticsOperation`. |
| `platform_matrix` | All 30 `(platform, operation)` rows. |
| `unresolved_host_adapters` | Qualification/host links. |
| `retention` | The active `RetentionPolicy`. |
| `recovery` | Full `RecoveryReport` (prior session, recovered crashes, retained counts, evictions, quarantined entries, unavailable evidence). |
| `events` | Retained redacted `DiagnosticEvent`s. |
| `crash_records` | Retained redacted `CrashRecord`s with distinct termination evidence. |
| `unavailable_evidence` | Explicit `UnavailableEvidence` markers and why they are unavailable. |

The export is self-describing and parseable without this crate's runtime. Tests
read the staged bytes through a `FileResource`/`ResourceReader`, parse the JSON,
and assert exact build identity, recovery summary, all six platforms, all five
operations, unavailable evidence, and zero redaction-canary leakage.

## Host FFI containment policy

These rules apply to every platform host that feeds diagnostics:

- Never unwind across a JNI, C ABI, Swift or Wasm boundary. Catch Rust panics at
  the Rust entrypoint and translate them to `TerminationKind::RustPanic`
  evidence and a typed `ErrorCode`.
- Leave native exception objects, JNI local/global refs, Objective-C/Swift
  objects and signal-handler-only state inside their owning boundary.
- A panic hook may copy only bounded, already-owned strings into a crash marker.
  A signal handler must be signal-safe: no allocation-heavy formatting, locks,
  Rust unwinding, JNI calls or blocking I/O. Prefer an async-signal-safe write
  to a pre-opened descriptor, then recover/promote on the next launch.
- Android ANR/tombstone/`ApplicationExitInfo` extraction happens in the JNI
  host, never by calling JNI from a signal handler. The host hands sanitized
  evidence to `write_crash_marker`.
- iOS MetricKit diagnostics are handed off from Swift after callback delivery;
  no Rust panic may cross the Swift boundary.
- Windows SEH and macOS/Linux signal boundaries capture the marker at the Rust
  entrypoint and convert it to a structured error/crash marker. Do not resume
  through a foreign frame with a Rust panic.
- Web has no process-level signal marker. Use browser lifecycle events and a
  host bridge; `WriteCrashMarker` is unsupported there.
- Hosts must not pass raw exception text, native paths, URLs or credentials in
  `SystemError` messages. All diagnostic evidence constructors redact anyway.

## Six-platform operation, configuration and evidence matrix

Programmatic APIs: `platform_support_matrix()` returns all 30 rows,
`operation_support(PlatformTarget, DiagnosticsOperation)` returns one row, and
`platform_support(PlatformTarget)` returns five rows. `PlatformOperationSupport`
uses `gpui_system::Support` and `Evidence` (`Native`, `Conditional`,
`Unsupported`, `ContractOnly`, `Compiled`, `RuntimeVerified`).

Legend: **N** = `Native`, **C** = `Conditional`, **U** = `Unsupported`.

| Platform | RecordEvent | WriteCrashMarker | InspectRecovery | ExportBundle | PruneRetained |
| --- | --- | --- | --- | --- | --- |
| Windows | N | C | N | N | N |
| macOS | N | C | N | N | N |
| Linux | N | C | N | N | N |
| Android | N | C | N | C | N |
| iOS | N | C | N | C | N |
| Web | C | U | C | U | C |

### Required host configuration

| Platform | Required configuration | Evidence / remaining work |
| --- | --- | --- |
| Windows x86_64 | Explicit private `AppData` root; panic hook + SEH/signal containment at Rust entrypoints; every storage/export call on a background context. | Compiled contract; packaged first executable path and runtime qualification pending [#2](https://github.com/jihuayu/gpui-box/issues/2), desktop hardware qualification [#8](https://github.com/jihuayu/gpui-box/issues/8). |
| macOS | Explicit private `AppData` root; panic hook + Mach exception/signal-safe marker handoff; background storage. | Compiled contract; desktop hardware qualification pending [#8](https://github.com/jihuayu/gpui-box/issues/8). |
| Linux | Explicit private `AppData` root; panic hook + signal-safe marker handoff; background storage. | Compiled contract; desktop hardware qualification pending [#8](https://github.com/jihuayu/gpui-box/issues/8). |
| Android arm64 | Host-injected app-private container root; JNI bridge for ANR/tombstone/`ApplicationExitInfo`; no unwinding across JNI; SAF/host handoff for export. | Compiled contract; native bridge and emulator acceptance pending [#2](https://github.com/jihuayu/gpui-box/issues/2), [#22](https://github.com/jihuayu/gpui-box/issues/22). |
| iOS arm64 | Host-injected app-private container root; MetricKit/signal-safe marker handoff; no unwinding across Swift/C ABI; document-picker/share-sheet export handoff. | Compiled contract; device qualification pending [#9](https://github.com/jihuayu/gpui-box/issues/9). |
| Web / WebWorker | Host bridge for ephemeral or IndexedDB-backed storage and explicit flush; browser download handoff for export; no filesystem `AppData` or `ExportResourceLease`. | Contract only; browser/Web backend maturity tracked with [#19](https://github.com/jihuayu/gpui-box/issues/19), [#22](https://github.com/jihuayu/gpui-box/issues/22), [#43](https://github.com/jihuayu/gpui-box/issues/43), [#73](https://github.com/jihuayu/gpui-box/issues/73). |

### Explicit unavailable collectors

`default_unavailable_evidence()` and `RecoveryReport.unavailable_evidence`
identify what this revision cannot observe:

- `native-signal-collector`: no SEH/signal/Mach collector is bundled; hosts must
  capture markers at their entrypoints.
- `jvm-anr-bridge`: Android ANR/tombstone/exit-reason evidence requires the JNI
  host bridge ([#22](https://github.com/jihuayu/gpui-box/issues/22)).
- `metrickit-crash-diagnostics`: iOS MetricKit evidence requires a Swift host
  handoff ([#9](https://github.com/jihuayu/gpui-box/issues/9)).
- `web-filesystem-export`: browsers have no filesystem `ExportResourceLease`.
- `runtime-renderer-trace`: renderer/GPU traces are backend-specific and not
  installed by this contract crate.

`PROVIDER_MANIFEST` remains the shared five-platform `gpui-system` manifest with
`native_provider_installed: false` and `Evidence::ContractOnly`. Web is included
in the operation matrix but not in that legacy five-platform manifest. A compile
is not runtime evidence; providers must publish per-operation runtime evidence
before promoting a row.

## Coordination links

- [#2](https://github.com/jihuayu/gpui-box/issues/2) — Windows/Android first
  executable path and packaged-host acceptance.
- [#8](https://github.com/jihuayu/gpui-box/issues/8) — macOS/Linux desktop
  hardware qualification.
- [#9](https://github.com/jihuayu/gpui-box/issues/9) — iOS device/MetricKit
  qualification.
- [#18](https://github.com/jihuayu/gpui-box/issues/18) — system integration
  roadmap.
- [#19](https://github.com/jihuayu/gpui-box/issues/19) — shared service
  contracts, capabilities and error vocabulary.
- [#22](https://github.com/jihuayu/gpui-box/issues/22) — Android JNI host bridge.
- [#23](https://github.com/jihuayu/gpui-box/issues/23) — authorized export
  leases and native handoff.
- [#43](https://github.com/jihuayu/gpui-box/issues/43) — background execution,
  durable retention and cancellation policy.
- [#68](https://github.com/jihuayu/gpui-box/issues/68) — this diagnostics slice.
- [#73](https://github.com/jihuayu/gpui-box/issues/73) — cross-platform
  diagnostics qualification and follow-up collector coordination.

## Service observations

`DiagnosticsService::subscribe()` returns a bounded single-consumer stream of
payload-free `DiagnosticServiceEvent` values. Success events report counts and
labels only; raw diagnostic values are never copied into the stream.

`DiagnosticServiceEvent::OperationFailed { operation, code }` is emitted exactly
once whenever a gated service operation returns `Err`. This includes:

- context gate failures (UI attachment, expired deadline);
- capability gate failures (unsupported, unavailable, unconfigured, denied);
- provider failures such as simulated disk-full;
- encoding, schema-validation or export staging failures.

Cooperative cancellation (`Ok(Completion::Cancelled(_))`) is not an error and
never emits `OperationFailed`. Tests assert this contract for a capability gate
error, a UI-context gate error, a simulated disk-full export failure, and a
cancelled operation.

## Optional GPUI adapter

The `gpui` feature adds:

- `impl gpui::Global for DiagnosticsService` and
  `install_diagnostics_service(service, cx)` / `diagnostics_service(cx)`.
- `install_diagnostics_in_services` for the typed `SystemServices` registry.
- Background spawn helpers (`spawn_record_event`, `spawn_recovery`,
  `spawn_export_bundle`, async form) that move only owned `Send` data to
  `cx.background_executor()`.
- `DiagnosticsEntityScope`, which retains observation streams alongside a
  `gpui_system::gpui_adapter::EntityScope` and invalidates the UI owner on
  entity release.

The adapter's `#[gpui::test]` integration test is compiled only for the `gpui`
feature and the three desktop targets whose GPUI test-support graph applies.

## Verification

From the repository root, with the shared target directory:

```sh
cargo metadata --format-version 1 > /dev/null
cargo fmt --all
cargo fmt --all -- --check
mise tasks validate
cargo test --locked -p gpui-diagnostics -p gpui-box
cargo test --locked -p gpui-diagnostics --all-features
cargo test --locked -p gpui-box --features diagnostics,diagnostics-gpui
cargo clippy --locked -p gpui-diagnostics -p gpui-box --all-targets -- -D warnings
cargo clippy --locked -p gpui-diagnostics --all-features --all-targets -- -D warnings
cargo clippy --locked -p gpui-box --features diagnostics,diagnostics-gpui --all-targets -- -D warnings
git diff --check
git status -sb
```

The behavioral suite covers all six domains and correlation fields; required
sensitive keys and string patterns; Unix/Windows/UNC paths; zero canary leakage
through `Debug` and JSON; count/byte/crash retention and oversized events; all
crash marker kinds and clean/unclean relaunch recovery; corrupt-file quarantine
with intact forensic bytes; simulated write/rename disk-full safety; UI,
deadline and cancellation ordering; capability gating before side effects;
cancelled export cleanup; and full bundle parsing through a `FileResource`.
