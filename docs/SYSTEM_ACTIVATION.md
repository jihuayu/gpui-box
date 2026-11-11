# Activation routing and URL opening

The `activation` facade feature exposes `gpui_box::activation` from the independent
`gpui-activation` package. It reuses shared errors, capability snapshots, scoped
requests and cancellation from #19 and private settings persistence from #22.
Enable `activation-gpui` for the optional coordinated GPUI adapter.

## Decisions and ownership

The durable inbox is a blocking **background primitive**, separated from UI-local
URL opening. Inbox open/enqueue/pending/acknowledge/expire reject UI-bound contexts.
Clone the inbox for concurrent background access; a nonblocking mutex serializes
updates. A per-namespace OS lock under `config/activation-locks` prevents separate
inbox objects sharing one AppData owner from doing conflicting load/save cycles.
This complements AppData's process/root ownership. Lock files are reserved and
must not be deleted while an owner is alive. No callback runs under inbox locks.

`ActivationTarget` has a registered route ID, positive schema version and bounded
string parameters. Notification targets reuse the same type; unregistered versions
produce a typed configuration error. This initial slice models explicit routes,
URLs, notification targets and plain activation. Authorized document resource
payloads require the resource contract in #23, and remain follow-up integration.
Native host/Intent/Scene callbacks and generated registrations stay with #2/#4/#5.

## Durable delivery

Hosts assign a stable `ActivationId` and reuse it for retries of one native event.
An ID must not be generated from the URL: distinct clicks on the same link may be
separate events. Enqueue validates the URL/route and persists before returning a
queued receipt. Failure or precommit cancellation leaves memory and committed
queue unchanged. Queued/acknowledged receipts expose #22's durability result.

`set_ready(true)` permits application consumption after services/routes/models
are initialized. The flag defaults to false each time the inbox is reopened.
Reset it before recreating an application host/model; queued data survives that
reset. `pending` returns a non-destructive snapshot, and business handlers execute
after the snapshot is returned. They must validate their current logical window
or Scene before navigation; this background inbox never retains a native owner.
A ready flag or target snapshot is not authorization to use a stale window.

Delivery is **at least once**: ack only after successful handling. A crash between
business handling and acknowledgment redelivers the same ID. Consumers must make
the business operation idempotent. Ack persists a bounded deduplication tombstone;
replayed pending/handled IDs return Duplicate. Tombstones expire or are evicted
oldest first when their configured bound is reached. Beyond that retention horizon,
the same ID may be queued again. Full pending queues return Busy and never silently
discard unhandled events. The host must retain/retry an event rejected as full.

Retention is based on the inbox's accepted UTC timestamp, not an untrusted event
payload timestamp. Wall-clock rollback retains future entries rather than deleting
them; a large forward change may expire them. Queue/dedup bounds remain enforced.
`expire` persists cleanup and returns its count; like ordinary cache cleanup it
does not expose the underlying directory-sync receipt. Do not infer power-loss
persistence guarantees from that count. Application events are bounded and stored
in private settings, never arbitrary diagnostics. Their URL/target Debug output
is redacted. Do not persist credentials or authentication tokens in event payloads.

Corrupt/unknown settings and unsupported route versions are preserved on open
failure. Recovery is explicit: after deciding how to retain/recover the events,
quarantine `app.settings(DataScope::Application, "activation-<namespace>", 1,
2 * 1024 * 1024)` and reopen. Quarantine retains original bytes; there is no silent
reset. Changing registered routes may require application-owned recovery/migration.

## URL execution

`UrlPolicy` requires an explicit bounded allowed-scheme set. URL parsing normalizes
using `url`, rejects control characters, oversized values, credentials, and web
URLs without an explicit authority. `file`, `javascript`, `data` and `shell` are
excluded: user documents use authorized resources, and executable URL schemes
need a separately reviewed provider. Applications remain responsible for which
custom schemes/hosts/routes they trust and for domain-specific URL parameter
validation. An allowed scheme does not make external content trusted.

`UrlOpening` holds an explicit local provider. Execution checks a current UI
owner/thread, cancellation/deadline, the fresh capability and conditions again
immediately before native invocation. No UI Future is moved to a worker. A
provider can return Submitted, which means it reports handoff, never successful
browser navigation. The optional direct GPUI adapter preserves `App::open_url`'s
void/best-effort semantics as AttemptedOutcomeUnknown. It does not invent a native
acceptance receipt, fallback or browser display evidence. User cancellation or
stale delivery after handoff cannot undo the native side effect.

## Platform evidence and outstanding work

| Platform | Current implementation | Evidence and remaining gate |
| --- | --- | --- |
| Windows | Durable inbox, explicit URL provider, GPUI best-effort bridge | Windows scratch-root runtime tests and adapter compile. Native custom protocol cold launch, packaged identity and protected single-instance forwarding remain #24/#4/#5/#8. |
| Android | Shared core with host-injected private storage | ARM64/x86_64 compile checks. Intent/onNewIntent and task behavior, registrations, runtime evidence remain #2/#24/#4/#5. |
| macOS | Portable contracts and GPUI bridge | No runtime qualification here; open events/Universal Links and protected forwarding remain #8/#24. |
| Linux | Portable contracts and GPUI bridge | No session/portal/desktop activation qualification here; #8/#24. |
| iOS | Shared contracts, host-injected private storage | Scene and Universal Links/device evidence remain #9/#24. |

`capabilities()` explicitly reports single-instance forwarding and native
registration Unsupported. No unauthenticated loopback IPC or implicit native
registration is substituted. Verified App Links/Universal Links require separate
native association configuration and evidence. There is no native installed
provider claim in this slice.

```sh
cargo test --locked -p gpui-activation
cargo clippy --locked -p gpui-activation --all-targets -- -D warnings
cargo check --locked -p gpui-box --features activation-gpui
cargo run --locked -p gpui-box --features activation,app-data --example activation
```

Behavior tests cover durable reopen, ready gating, pending/acknowledged dedup,
namespace ownership, overflow, retention/clock rollback, malformed URLs/routes,
redaction, cancellation, corrupted data and explicit quarantine, failed persistence
without in-memory acceptance, denied/unavailable/configuration errors, UI-thread
validation and stale request delivery. Fault evidence uses an actual failing
filesystem target, not a physical power-loss test. The sample consumes only public
facade APIs and does not launch an external browser.
