# Private application data and settings

`gpui-box` feature `app-data` exposes `gpui_box::app_data`. The independent
`gpui-app-data` package uses the common `gpui-system` errors, cancellation and
operation capability contracts. It has no GPUI or Android dependency. Register a
clone of `AppData` with `SystemServices` to share it between application models;
the registry stays on the UI thread while owned `AppData` clones are Send.

## Execution and ownership

All filesystem APIs are **blocking background primitives**. Create/open storage,
read, migrate, write and clean up on a worker. In a GPUI application, clone the
service and an immutable value, then use `cx.background_executor().spawn(async
move { store.save(&value, &context) })`. Return the result through the foreground
owner. `OperationContext` must have no UI attachment: storage does not present
native UI, and rejects UI contexts instead of moving them across threads.
Cancellation and deadlines are checked at entry and before atomic replacement.
A cancellation after commit cannot undo the write. These primitives do not
interrupt an in-progress OS filesystem call.

`AppData::open` creates/resolves private roots and takes a nonblocking OS file lock
in **every canonical purpose root**, including control, configuration, data,
cache, logs and temporary. Equal or ancestor/descendant roots are rejected. A
second service/process sharing any root gets `Busy`; changing only control cannot
bypass ownership. Clone the service for multiple windows. Clones serialize with a
nonblocking mutex; reentrant codecs/migrations return `Busy`, avoiding deadlock.
OS locks are released on process termination. `.owner.lock` is reserved and is
never removed by cleanup. This is cooperative ownership, not a security sandbox.

Roots must be trusted, application-private locations. `AppDirectory::child`
rejects absolute paths, parent traversal and existing symlink children, but its
lexical checks are **not** race-proof against hostile filesystem mutation.
Consumers must not hand the directories to untrusted writers or delete lock
files. Filesystem ACLs, platform sandbox and mobile protection/backup attributes
remain host responsibilities. Native desktop resolution uses `dirs` known
folders/XDG conventions without changing native permissions. The `sandboxed`
policy is a supplied declaration, not a detected guarantee. Private roots may
contain spaces and non-ASCII characters.

## Settings and recovery

`settings(scope, name, schema, max_bytes)` returns a codec-independent
`SettingsStore`. Scope is application-wide or an account namespace (up to 96 UTF-8
bytes, encoded as a collision-free path component). Settings names are bounded
portable identifiers and filenames have a fixed prefix. The JSON envelope has an
explicit format and positive schema version. Reads are bounded to at most 16 MiB.
Settings are not a credential store; no contents or paths appear in public errors.

Save checks the existing envelope, stages a same-directory temporary file, writes
and syncs its contents, rechecks cancellation/deadline, then atomically replaces
with `tempfile::persist`. The target is old or new when replacement is atomic on
the underlying filesystem. Network/virtual filesystems require their own
qualification. The receipt reports `FileSynced` on Windows (directory persistence
across sudden power loss is not promised), `FileAndDirectorySynced` on Unix when
directory sync succeeds, or `CommittedDirectorySyncUncertain` if Unix directory
sync fails **after** replacement. A committed write never returns a misleading
precommit failure. OS errors are typed and retryable after conditions change.

A mismatched schema requires explicit `migrate`; each step converts vN to vN+1 on
a detached JSON value, and all successful steps are committed in one replacement.
A failed step/write preserves the old file. Downgrades are rejected. Migration
callbacks must be bounded; reentrant storage calls return `Busy`. Corrupt,
oversized and unknown-format data remains untouched. Save also refuses to replace
it. `quarantine` is an explicit caller decision that retains the original under a
unique name before a later reset/save. Settings codec validation failures do not
automatically delete/reset data. A crash can leave ignored staging files, which
are never treated as committed settings.

`remove_account` is idempotent and touches only that account's config/data scope.
Discard outstanding store handles for the account before logout cleanup. Partial
I/O failures can leave some selected-account files present; retry cleanup after
fixing the cause. Other accounts and application settings remain separate.
Cache eviction clears only cache content and retains its reserved lock. It never
touches configuration, data or exports.

## Export leases

Temporary exports are size-bounded files in the temporary purpose root. Retain
`ExportLease` until the native consumer has finished using its path; the lease
also retains AppData process ownership. Dropping it releases only its pin, with
no blocking cleanup. `cleanup_exports(minimum_age)` only removes unleased regular
files with this service's export prefix, including abandoned files after process
restart. Age **never overrides an active lease**. Unknown files and symlinks are
preserved. The next cleanup prunes stale bookkeeping. Paths/native grant creation
are the responsibility of the authorized files/sharing providers (#23/#25).
There is no claim that a native consumer automatically extends a Rust lease.

## Runnable example and evidence

```sh
cargo run --locked -p gpui-box --features app-data --example settings
cargo test --locked -p gpui-app-data
cargo clippy --locked -p gpui-app-data --all-targets -- -D warnings
```

The example uses temporary roots and performs save, schema migration and reload
through the public facade. Tests use scratch directories only, including a path
with spaces/non-ASCII. Windows evidence covers relaunch, migration rollback,
corruption quarantine, invalid schema/size, precommit disk-full/interruption fault
injection, cancellation, overlapping/escaping roots, changed-control lock bypass,
actual subprocess lock contention, account isolation and export/cache lifetime.
The fault tests simulate storage-full/interruption; they do not claim a full
physical volume or power-cut test.

| Platform | Directory provider | Evidence / remaining work |
| --- | --- | --- |
| Windows | Known folders via dirs; explicit roots supported | Runtime tests on Windows x86_64 using temporary roots; native resolution read-only smoke test. Packaged identity/ACL and abrupt-power-loss qualification pending #4/#5/#8. |
| Android | Host application-context no-backup roots with separate cache | `gpui_box::android::app_data_roots` resolves on workers after Activity host setup. Two-ABI compilation and emulator storage qualification are tracked under #2/#22. No inferred desktop fallback. |
| macOS | dirs Application Support/config/cache mapping | Contract only; sandbox/container and runtime qualification pending #8. |
| Linux | dirs XDG mapping | Contract only; environment/filesystem/runtime qualification pending #8. |
| iOS | Host-injected container roots and policy | Contract only; backup/protection attributes and device qualification pending #9. |

Capabilities describe implemented operations and configuration for an opened
provider; their `Compiled` evidence deliberately does not promote any native
platform qualification to runtime verified. Native mobile root discovery without
host configuration returns `InvalidConfiguration`. Backup archives, database
transactions, live provider callbacks, migration backups and UI gallery workflows
remain separate work; this slice provides recoverable versioned settings and a
persistence adapter rather than a general database.

## Android application-context roots

The Android Activity installs its application context into NativeBridge before
surface/application launch. `gpui_box::android::app_data_roots()` attaches the
calling worker to the JVM and queries that context. It returns existing AppData
DirectoryRoots and exposes the same storage types through
`gpui_box::android::app_data`. Perform discovery, `AppData::open`, settings and
cleanup on a worker. Retain one opened AppData owner and clone it for users.
Discovery on the main Looper is rejected. Missing host context returns
Unavailable. JNI query exceptions are cleared and mapped to a typed error.

Control/config/data/logs/temporary use disjoint children of
`Context.noBackupFilesDir/gpui-box`. Cache uses `Context.cacheDir/gpui-box`.
Temporary export leases use no-backup storage so OS cache eviction cannot remove
an active export. Explicit export cleanup remains the existing AppData policy.
The native roots carry HostManaged backup policy and sandboxed application
ownership. AppData labels cache/temporary as Regenerable. The current generated
host also disables application backup. No-backup files are excluded from Android
Auto Backup. Device transfer, OEM behavior, credential-encrypted locked-boot
availability, uninstall/reinstall retention and backup/restore journeys retain
separate qualification. These roots become available after ordinary Activity
creation and provide credential-encrypted application storage.

The bridge retains application context across Activity recreation, with no
Activity reference or surface dependency. Discovery leaves directory creation,
canonical containment and lifetime locking to AppData. Platform directory getters
can perform I/O, so discovery also belongs on the worker. The shared registry may
receive a cloned opened handle through the application's foreground owner. Native
roots are application-scoped. Existing DataScope handles account partitioning.

The API35 Android15 x86_64 emulator runs the example-only JNI storage probe on
the instrumentation worker. It opens real host roots, saves Chinese/emoji JSON,
releases lifetime locks, reopens and reloads settings, evicts a cache file while
preserving a leased export, then explicitly removes the released export. Two
separate instrumentation process launches retain and increment saved visits
(2 then4 before application-data reset). The full suite passes22/22 in69.168s.
Both ARM64/x86_64 native builds and strict Android Clippy pass. The test bridge
lives only in the Android example. Generated applications use the public roots
API on their own workers. Actual backup, device transfer, hardware storage and
direct-boot behavior retain the gates described above.
