# Persistent local notification scheduling and reconciliation

This document specifies `crates/gpui-notification-schedule` for
[#44](https://github.com/jihuayu/gpui-box/issues/44). The crate coordinates
durable local notification intent with the notification content from
[#26](https://github.com/jihuayu/gpui-box/issues/26) (`gpui-notifications`),
permission and special-access state from
[#20](https://github.com/jihuayu/gpui-box/issues/20) (`gpui-permissions`),
private application storage from
[#22](https://github.com/jihuayu/gpui-box/issues/22) (`gpui-app-data`) and
durable activation routing from
[#24](https://github.com/jihuayu/gpui-box/issues/24) (`gpui-activation`). The
execution and provider contracts are shared with
[#54](https://github.com/jihuayu/gpui-box/issues/54) (`gpui-jobs`).

The default build has no GPUI dependency. It installs no native scheduling
provider and never claims that a notification was presented merely because it
was accepted for scheduling. Every cell in the platform matrix is
`Evidence::ContractOnly`; `Native` or `Conditional` in the support column
describes the intended host mechanism, not compiled or runtime-qualified
evidence.

## Scope and ownership

`gpui-notification-schedule` owns:

- Strict schedule identity (`ScheduleNamespace` + application name), bounded
  names, hex SHA-256 storage ids that cannot traverse a path, and a distinct
  activation notification id.
- One-shot absolute, relative-delay, fixed-interval and civil-calendar triggers
  with integer-only validation.
- Built-in timezone resolution for UTC, `Etc/UTC`, `Asia/Tokyo`,
  `Asia/Kathmandu`, `America/New_York`, `Europe/London`, `Europe/Berlin` and
  `Australia/Sydney`, including explicit DST gap and fold policies. Named-zone
  DST transitions are tabulated for 2024 through 2035.
- A versioned durable journal with a SHA-256 envelope, atomic staged commits,
  semantic validation and evidence-preserving quarantine.
- Monotonic per-record `schedule_epoch` fencing across cancellation,
  replacement and process restart.
- Strictly separated submission, presentation and activation evidence.
- Capability and permission gating, explicit exact-alarm downgrade or rejection,
  channel/policy suspension and re-admission.
- Restart, forward-clock, backward-clock, timezone, DST, permission and channel
  reconciliation with exactly one provider registration per changed schedule.
- Bounded retention for active and terminal records.
- A six-platform, thirty-six-operation qualification matrix and a Web
  capability policy that never assumes baseline notification triggers.
- A bounded `EventSubscription<NotificationScheduleEvent>` using
  `gpui_system::event_channel` (capacity 64).
- An optional GPUI adapter with an `App` global, `SystemServices` registration,
  an exactly generation-fenced surface scope and a bounded model.

It does **not** own:

- Native Windows `ScheduledToastNotification`/`UserNotificationListener`, macOS
  or iOS `UNUserNotificationCenter`, Linux systemd user timers or the XDG
  Background Portal, Android `AlarmManager`/`NotificationListenerService`, or
  browser Service Worker notification triggers. Those host adapters remain
  tracked under [#8](https://github.com/jihuayu/gpui-box/issues/8),
  [#2](https://github.com/jihuayu/gpui-box/issues/2),
  [#9](https://github.com/jihuayu/gpui-box/issues/9) and
  [#43](https://github.com/jihuayu/gpui-box/issues/43).
- Rendering, notification UI, action routing or deep-link handling. Activation
  is handed to `gpui-activation` as an `ActivationEvent`.
- Encryption at rest or backup policy. The namespace lives below the private
  application data root and inherits `gpui-app-data` policy.
- Timezone database updates. Named zones are a bounded built-in table; a host
  may inject a richer `TimeZoneResolver`.

## Crate layout

| Module | Responsibility |
| --- | --- |
| `calendar` | Proleptic Gregorian integer arithmetic, `Weekday`, `CalendarRule`, leap-year and civil-range validation. |
| `timezone` | `TimeZoneSpec`, `ResolvedTimeZone`, DST transition tables, gap/fold resolution, next-occurrence scanning. |
| `model` | Identity, triggers, policies, content attachment model, durable record, events, retention, receipts and invariant validation. |
| `clock` | `ScheduleClock`, system wall clock and deterministic fake clock. |
| `platform` | Six operations × six platforms, contract evidence, host configuration notes, issue links and the Web policy. |
| `provider` | `NotificationScheduleProvider` boundary, capability checks, manifests, fail-closed provider and deterministic fake provider. |
| `store` | Private namespace, envelope framing, checksum, atomic writes, quarantine and fault-injection hook. |
| `service` | Submission, evidence recording, cancellation, reconciliation, retention and event delivery. |
| `gpui_adapter` | Optional GPUI global, service registration, surface scope and bounded model. |

The facade is enabled explicitly:

```toml
gpui-box = { features = ["notification-schedule"] }
# Adds the GPUI adapter, App global and model:
gpui-box = { features = ["notification-schedule-gpui"] }
```

`gpui_box::notification_schedule` re-exports the crate. The feature wiring is
`notification-schedule = ["system", "app-data", "permissions", "activation",
"notifications", "dep:gpui-notification-schedule"]`; the `-gpui` variant adds the
GPUI features of every sibling crate and `gpui-notification-schedule/gpui`.

## Identity, epochs and concurrency

`ScheduledNotificationId` combines a `ScheduleNamespace` with a bounded
application-provided name. The durable file id is the lowercase hex SHA-256 of a
domain-separated identity encoding; the activation notification id is a
separate stable string. Names may not contain path separators, control
characters or traversal components, and every id is bounded before hashing.

Every record carries a non-zero `schedule_epoch`. Epochs are allocated
monotonically:

- A new schedule, a `Replace` submission and a cancellation advance the
  service epoch.
- The highest epoch on disk seeds allocation at `open`, so a process that
  submits before its first reconciliation cannot reuse an epoch that is still
  durable. `rehydrate` also advances the watermark.
- Provider requests and receipts carry the epoch. Delivery and activation
  observations must match the current epoch; a cancellation or replacement
  makes older callbacks fail with `ErrorCode::ContextExpired` before any
  mutation.
- Provider tokens are informational. The epoch, not a platform token, is the
  authoritative fence.

All durable and provider work requires `ExecutionOwner::Background`. The
service rejects a context with `OperationContext::ui = Some(_)` (and any
caller-declared `Renderer` owner) with `ErrorCode::InvalidInput` before opening a
file or calling the provider. `OperationContext::check_at` and a cancelled
`CancellationToken` are checked before each side effect, including after
acquiring the operation lock. Provider and event callbacks are invoked after
the state mutex is released. Poisoned internal mutexes are recovered with
`PoisonError::into_inner` so teardown cannot panic.

## Triggers, calendar rules and DST

`ScheduleTrigger` is one of:

| Trigger | Semantics |
| --- | --- |
| `OneShotAt { fire_at_epoch_ms }` | Absolute Unix-epoch milliseconds; must be strictly in the future at submission. |
| `AfterDelay { delay_ms }` | Strictly positive delay; resolved once to absolute `submitted_at + delay`. |
| `RepeatingInterval { interval_ms, max_occurrences }` | Interval is at least 60,000 ms. Occurrences are arithmetic and skip missed slots exactly. |
| `CalendarRule(CalendarRule)` | Civil fields resolved in `TimeZoneSpec`; optional year, month, day, weekday, hour, minute, second and max occurrences. |

Calendar validation rejects impossible dates (for example February 30), years
outside 1970–9999, hours outside 0–24 and minute/second outside 0–59. February
29 is accepted when a future leap year exists; a fixed non-leap year with no
occurrence is rejected at submission. A weekday-only rule means “the next such
weekday at the given civil time”; year/month/day are optional filters.

Time zones are `Utc`, `FixedOffsetMinutes` in −840..=840, or a bounded `Named`
id. Named resolution is case-sensitive and fail-closed for unknown ids. Gap and
fold behavior is explicit, never inferred:

- `DstGapPolicy::ShiftForward` moves a nonexistent local time to the transition
  instant (for example 02:30 in the New York spring gap becomes 03:00 EDT).
- `DstGapPolicy::SkipOccurrence` advances to the next matching civil date.
- `DstFoldPolicy::FirstOccurrence` selects the pre-transition offset.
- `DstFoldPolicy::SecondOccurrence` selects the post-transition offset.

`following_active_time_zone(true)` makes a calendar rule use
`ScheduleClock::active_time_zone()` and stores the resolved zone with the
record. Reconciliation recomputes calendar triggers on every pass, so a
timezone change, DST table update or backward wall-clock shift re-registers the
correct next occurrence on the next clock/timezone reconciliation pass. One-shot, delay and interval triggers stay absolute;
a clock jump only coalesces their missed occurrences.

## Three evidence stages

The crate refuses to turn one kind of evidence into another.

| Stage | Input | What it proves | What it never proves |
| --- | --- | --- | --- |
| Submission | `ScheduleSubmission` | Validation, capability/permission admission, durable intent and provider acceptance. Returns `ScheduleSubmissionReceipt`. | That the OS displayed anything or that a user saw it. |
| Presentation | `DeliveryObservation` | A platform callback reported an actual displayed occurrence at a specific epoch/fire time. | User action, action id, text input or routing. |
| Activation | `ActivationObservation` | A user tap or declared action/text-input invocation, persisted as `ScheduledActivationHandoff`. | That the notification was presented; presentation counts remain unchanged. |

Submission receipts expose `disposition` (`Created`, `Replaced`,
`KeptExisting`, `DowngradedToInexact`), precision, epoch, provider token,
next fire and an optional downgrade reason. A `KeepExisting` submission is
idempotent and does not call the provider again. `RejectConflict` fails with
`InvalidInput`; `Replace` advances the epoch and asks the provider to atomically
upsert the same identity at the newer epoch. The provider contract makes the
older epoch stale.

`record_delivery_observation` accepts only `Submitted` or `Scheduled` records
with the current epoch. It requires the `ObserveScheduledDelivery` capability,
appends one bounded observation, updates presentation counters and the
last-presented timestamp, then either advances the repeating/calendar trigger
and resubmits the next occurrence or terminates the record. A terminal record is
cancelled at the provider and persisted with `Delivered`, `Completed` or
`Expired`; it can never accept another delivery.

`record_activation` validates the current epoch and, when the content declares
actions, requires the invoked action to be declared. Text input is accepted only
for the reserved `schedule-text-input` action, is bounded to 1024 bytes, must be
trimmed non-empty and may not contain control characters. A body tap may not
carry text. The returned handoff retains the action id and text input separately
and exposes an `ActivationPayload::Notification` plus an `ActivationEvent` for
`gpui-activation`.

## Capabilities, exact alarms and attachments

`NotificationScheduleProvider::capability(operation)` returns a
`gpui_system::Capability` with `Support`, `Availability`, `Configuration`,
`Evidence` and recovery advice. The service checks the shared trait for each
operation:

| Operation | Submission gate |
| --- | --- |
| `ScheduleNotification` | Required for every submission. |
| `RepeatAndCalendarSchedule` | Required for repeating and calendar triggers. |
| `ScheduleExactAlarm` | Required only for `SchedulePrecision::ExactAlarm`. |
| `AttachMediaToSchedule` | Required when attachments are present. |
| `ObserveScheduledDelivery` | Required before recording presentation. |
| `ReconcilePendingSchedules` | Required for startup, clock/timezone and permission reconciliation. |

Exact alarm additionally requires the special access permission
`PermissionKind::SpecialAccess("schedule_exact_alarm")`. Admission is:

- Capability and permission allow use: `ExactAlarm`.
- Either is missing and the fallback is `AllowInexact`: the record is persisted
  as `InexactAdvisory` with an explicit `InexactDowngrade` reason
  (`ExactAlarmCapabilityUnavailable` or `ExactAlarmPermissionDenied`), and the
  event `DowngradedToInexact` is emitted.
- Either is missing and the fallback is `Reject`: fail closed with
  `Unsupported` or `PermissionDenied` before provider scheduling.

Attachments are bounded to four items, 4 MiB each and 8 MiB total. Only bundled
assets and private local files are accepted; remote URLs, absolute paths,
traversal and control characters are rejected before a provider call. MIME
types are bounded and must be well formed. `gpui-notifications` has no
attachment model of its own, so the schedule record owns this validated list
and maps it to the host at the provider boundary.

## Durable store and quarantine

The journal is stored at:

```text
<AppData::Data>/notification-schedule/schedule-<sha256-hex>.json
```

Each file is one envelope:

```json
{
  "schema_version": 1,
  "checksum": "<64 lowercase hex characters>",
  "payload": { "...": "NotificationScheduleRecord" }
}
```

The checksum is SHA-256 over the domain separator
`b"gpui-notification-schedule-envelope\0"`, the little-endian `u32` format
version, and `serde_json::to_vec(payload)` serialized as the exact structurally
valid payload. Unknown envelope fields are rejected. One record is limited to
256 KiB of payload and 260 KiB of framed bytes.

Write protocol:

1. Validate the record semantically before serializing.
2. Create a uniquely named staging file in the same private directory.
3. Write all bytes, flush, invoke the optional `ScheduleStorageHook`, and
   `sync_all` the staging file.
4. Atomically `persist`/rename over the destination.
5. Update in-memory state only after the durable commit succeeds.

A failed write or rename leaves the prior file and in-memory record untouched.
Submission first persists a `Submitted` intent, then asks the provider; a
provider failure deletes the intent and does not leave a phantom active record.
Replacement persists the new record before asking the provider to upsert the
same identity, so a crash cannot lose the source of truth; the previous epoch is
stale by contract.

Load protocol:

- Missing directory is an empty journal.
- Files whose name does not match the schedule pattern are ignored.
- Symlinks, non-files, oversized files, malformed JSON, unknown schema versions,
  checksum mismatch, storage-id mismatch and records that fail `validate()` are
  moved to `notification-schedule/quarantine/<original>.<sequence>.corrupt`.
- Original bytes are preserved; quarantine failures never panic and never abort
  loading healthy siblings. Each quarantined record emits
  `CorruptRecordQuarantined` and increments the reconciliation report.

`ScheduleStorageHook` lets a host or test inject deterministic write/rename
failures without touching private internals. `ScheduleStorageFaultInjection`
provides `fail_next_write`, `fail_next_rename`, `truncate_next_write` and
`corrupt_next_write` for failure-path tests.

## Reconciliation

All three reconciliation entry points are background-only, hold the operation
lock, check cancellation, and emit events only after releasing the state lock.
Every provider call is bounded to one per record that changed.

### Startup

`reconcile_startup` rehydrates the journal; it does **not** auto-prune
terminal records. Retention pruning is an explicit `prune` /
`prune_with_policy` call. It compares active records with
`provider.pending_schedules()` and:

| Condition | Action |
| --- | --- |
| Active record missing from the provider | Re-submit with the same durable epoch. |
| Provider registration has a different epoch | Cancel the stale registration, then re-submit the durable record. |
| Provider registration has no active record | Cancel it as an orphan. |
| Record is terminal or suspended | Ensure no pending provider registration remains. |
| Corrupt journal file | Quarantine, report and continue. |

The report exposes journal/active counts, in-sync, resubmitted, cancelled
orphans, suspended, provider failures and quarantined records. Provider
failures are counted, never converted into success.

### Clock and timezone changes

`reconcile_clock_or_timezone_change` compares the record's next fire with the
current wall clock and applies `MissedFirePolicy`:

| Policy | Missed one-shot | Missed repeating/calendar |
| --- | --- | --- |
| `FireOnceLatest` | One catch-up fire at `now`, if grace permits. | One catch-up fire at `now` plus the next future occurrence; never one call per missed interval. |
| `SkipToNext` | Advance without firing. | Advance to the next future occurrence without firing. |
| `ExpireAfterGrace { grace_ms }` | Fire at `now` when the delay is within grace, otherwise `Expired`. | Same grace decision, then advance or expire. |

Catch-up is represented durably with `catch_up_at_epoch_ms`; the provider is
registered at the catch-up instant and the next regular occurrence remains in
`next_fire_at_epoch_ms`. A seven-day jump over 64 one-minute schedules performs
64 provider calls, not 9,216. If no future occurrence exists, the record becomes
`Expired`, the provider registration is withdrawn and a terminal event is
emitted.

Calendar triggers are recomputed on every reconciliation even when their
previous fire is in the future. This handles local timezone changes, DST rule
changes and backward wall-clock shifts. Absolute triggers are not rewritten by a
clock change; only their missed-fire policy applies. Updated timestamps are
clamped to the prior value so a backward clock cannot violate the record's
`updated_at >= submitted_at` invariant.

### Permissions and channels

`reconcile_permissions_and_channels` evaluates each active record in order:

1. Missing notification permission or a disabled/muted channel suspends the
   record with `NotificationsPermissionDenied` or `ChannelDisabled`, withdraws
   the provider registration and persists the suspension.
2. A revoked exact-alarm permission downgrades an `AllowInexact` record to
   `InexactAdvisory` and resubmits it inexactly; a `Reject` record is suspended
   with `ExactAlarmPermissionRevoked`.
3. A suspended record is re-admitted only after the blocking policy is gone and
   any required exact-alarm permission is restored.
4. Repeating the pass is idempotent: no duplicate downgrade, suspension or
   provider churn.

Suspension preserves `next_fire_at_epoch_ms`, clears the provider fire/token
fields and leaves no OS registration. Re-admission re-submits the preserved
fire and persists the resulting token before the record becomes live again.

## Retention

`RetentionPolicy` is validated before use:

| Field | Default | Bound |
| --- | --- | --- |
| `max_active_schedules` | 64 | 1..=1,024 |
| `max_terminal_records` | 128 | 0..=4,096 |
| `terminal_ttl_ms` | 7 days | 1 ms..=365 days |

The active budget counts all non-terminal records, including
`SuspendedByPolicy`, excluding the record being replaced. Exceeding it fails
with `ErrorCode::Busy`. `prune` removes terminal records older than
the TTL, then the oldest remaining terminal records beyond the terminal budget,
deleting both the durable file and the in-memory entry. Active records are never pruned. The report exposes removed and remaining
terminal counts.

## Platform matrix and Web policy

The matrix has exactly 36 cells (`6 platforms × 6 operations`). Every row is
`Evidence::ContractOnly` and carries the host configuration that would be
required, notes and links to the relevant platform/coordination issues. The
support values are:

| Platform | Schedule | Exact alarm | Repeat/calendar | Attach media | Observe delivery | Reconcile |
| --- | --- | --- | --- | --- | --- | --- |
| Windows | Native | Unsupported | Conditional | Conditional | Conditional | Native |
| macOS | Native | Unsupported | Conditional | Conditional | Conditional | Native |
| Linux | Native | Unsupported | Conditional | Conditional | Unsupported | Conditional |
| Android | Native | Conditional | Conditional | Conditional | Conditional | Conditional |
| iOS | Native | Unsupported | Conditional | Conditional | Conditional | Native |
| Web | Conditional | Unsupported | Conditional | Conditional | Unsupported | Conditional |

`Native`/`Conditional`/`Unsupported` describe intended host mechanisms only.
No native provider is installed by this crate; `UnavailableNotificationScheduleProvider`
reports `ContractOnly` capability with `Support::Unsupported` for every
operation and fails closed. `PROVIDER_MANIFEST` contains the five non-Web
provider contracts, all `ContractOnly`; `durable_scheduling_available` is false
for every platform.

`WebNotificationSchedulePolicy` is explicit for every maturity level
(`Experimental`, `Preview`, `Stable`):

- A Service Worker is required.
- Notification Triggers are optional and **not** baseline.
- Scheduled delivery is not observable.
- Exact alarms are unsupported.
- `ScheduleNotification`, repeating/calendar and attachment work are
  `Conditional`; delivery observation and exact alarms are rejected.
- Repeating/calendar work is app-computed and re-registered; IndexedDB/OPFS
  records may be evicted, so reconciliation cannot assume durability.

Per-platform issue links in every matrix row include the shared contracts and
coordination issues: `#19`, `#20`, `#21`, `#22`, `#24`, `#26`, `#43`, `#44`,
`#54` and `#70`, plus `#8` for desktop, `#2` for Android and `#9` for iOS.

## GPUI adapter and events

With the `gpui` feature:

- `install_notification_schedule_service(service, cx)` installs the `App`
  global.
- `init_notification_schedule(cx, provider, clock, resolver, permissions)`
  builds a private in-memory service and installs it as the `App` global and in
  `SystemServices`.
- `install_notification_schedule_in_services(&mut services, service)` performs
  only the `SystemServices` registration.
- `SurfaceNotificationScheduleScope::for_entity` and `::new` retain a
  `UiOwner` and fence disposal to the exact `(SurfaceId, generation)` that
  created them. Dropping a stale scope cannot dispose a newer surface
  generation. `SurfaceNotificationScheduleScope::context()` returns a
  UI-attached `OperationContext` (`ui: Some(ui)`) for generation fencing;
  durable scheduling operations require a background `OperationContext`
  (`ui: None`), such as `OperationContext::default()` on a background owner.
- `NotificationScheduleModel` subscribes to the bounded event channel, exposes
  at most 64 recent events, sets a sticky `resync_required` flag on overflow and
  provides `drain_events`. It does not perform durable work on the UI thread.

`NotificationScheduleEvent` variants are `Submitted`, `Scheduled`, `Presented`,
`Activated`, `DowngradedToInexact`, `Suspended`, `Resumed`, `Cancelled`,
`Expired`, `Completed`, `Rescheduled`, `CorruptRecordQuarantined` and `Pruned`. Submission, presentation
and activation are never collapsed into one variant. Overflow on a slow
subscriber yields `Event::ResyncRequired`, which the model surfaces to the host.

## Coordination

| Issue | Contract used here |
| --- | --- |
| [#19](https://github.com/jihuayu/gpui-box/issues/19) | `OperationContext`, `Capability`, `Support`, `Evidence`, `SystemError`, events, services. |
| [#20](https://github.com/jihuayu/gpui-box/issues/20) | Notification permission and `SpecialAccess("schedule_exact_alarm")`. |
| [#21](https://github.com/jihuayu/gpui-box/issues/21) | Surface/generation ownership used by the optional adapter. |
| [#22](https://github.com/jihuayu/gpui-box/issues/22) | Private `DataLocation::Data` namespace, atomic storage patterns. |
| [#24](https://github.com/jihuayu/gpui-box/issues/24) | `ActivationPayload::Notification`, `ActivationSource::Notification`, `ActivationEvent`. |
| [#26](https://github.com/jihuayu/gpui-box/issues/26) | `NotificationContent`, actions, activation bindings and channel ids. |
| [#43](https://github.com/jihuayu/gpui-box/issues/43) | Web backend maturity and capability policy. |
| [#44](https://github.com/jihuayu/gpui-box/issues/44) | This crate's contract and acceptance. |
| [#54](https://github.com/jihuayu/gpui-box/issues/54) | Background ownership, poison-safe locking and deterministic fake-clock test patterns. |
| [#70](https://github.com/jihuayu/gpui-box/issues/70) | Shared provider/capability coordination. |

The provider boundary deliberately mirrors `gpui-jobs`: a receipt proves
acceptance only, pending enumeration is authoritative only for the app's own
registrations, and every discrepancy is repaired from the durable journal rather
than from inferred OS state.

## Build and versions

- Workspace package version: `0.1.0-dev`.
- Rust edition 2024, toolchain pinned by `mise.toml` to `1.98.1`.
- Coordinated upstream GPUI revision:
  `f8cfc7ed9e57503def9479138894f3139f5400cc`.
- Direct runtime dependencies: `gpui-system`, `gpui-app-data`,
  `gpui-permissions`, `gpui-activation`, `gpui-notifications`, `serde` 1,
  `serde_json` 1, `sha2` 0.10 and `tempfile` 3.27. `gpui` is optional.
- No floats, sleeps, polling timers, `unsafe`, `println!`, `dbg!`, `todo!` or
  `unimplemented!` are used in the crate.

## Verification

Run from the repository root:

```bash
cargo fmt --all -- --check

mise tasks validate

cargo test --locked -p gpui-notification-schedule --all-features

cargo test --locked -p gpui-box \
  --features notification-schedule,notification-schedule-gpui \
  --test notification_schedule --lib

cargo clippy --locked -p gpui-notification-schedule -p gpui-box \
  --all-features --all-targets -- -D warnings

! grep -rn "/home/$USER" \
  crates/gpui-notification-schedule crates/gpui-box \
  docs/S_NOTIFICATION_SCHEDULE.md Cargo.toml mise.toml
```

The last command expands `$USER` to the current account and must produce no
matches; this document intentionally does not embed a developer-specific
absolute home path.

The test suite covers 29 crate unit tests plus 56 integration tests for triggers
and DST, the three evidence stages, durable persistence/quarantine/rollback,
restart, clock and permission/capability reconciliation, retention and the
platform matrix.
The facade test adds one library test and four integration tests, including the
GPUI adapter and a durable restart.

## Unresolved host adapters

The following are intentionally outside this crate and remain host work:

| Platform | Missing adapter | Tracking |
| --- | --- | --- |
| Windows | `ToastNotifier.AddToSchedule`/`GetScheduledToastNotifications`, `UserNotificationListener` presentation evidence, AUMID/packaged identity. | [#8](https://github.com/jihuayu/gpui-box/issues/8) |
| macOS | `UNUserNotificationCenter` add/pending/remove, delegate presentation and activation, notification authorization. | [#8](https://github.com/jihuayu/gpui-box/issues/8) |
| Linux | systemd `--user` timers or XDG Background Portal, freedesktop notification hints, no portable pending enumeration or delivery receipt. | [#8](https://github.com/jihuayu/gpui-box/issues/8) |
| Android | `AlarmManager` exact/inexact alarms, `NotificationListenerService`, `POST_NOTIFICATIONS`, exact-alarm special access, channel policy. | [#2](https://github.com/jihuayu/gpui-box/issues/2) |
| iOS | `UNUserNotificationCenter` add/pending/remove and delegate callbacks, authorization and the 64-request cap. | [#9](https://github.com/jihuayu/gpui-box/issues/9) |
| Web | Service Worker registration, optional non-baseline Notification Triggers, IndexedDB/OPFS journal, eviction and permission handling. | [#43](https://github.com/jihuayu/gpui-box/issues/43) |

Until those adapters are installed and qualified, the service reports
`ContractOnly` evidence, refuses to claim presentation, and reconciles only
against the durable journal and the injected provider contract.
