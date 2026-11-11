# M5 update trust and orchestration

## Implemented core

`gpui-updates` provides dependency-light signed metadata verification, a durable
replay/time checkpoint and generation-correlated orchestration. Enable the
facade's `updates` feature to use it as `gpui_box::updates`. Native adapters
publish per-operation platform/channel/evidence manifests. The crate's default
manifest records ContractOnly and an absent native adapter.

ReleaseProfile and UpdateChannel are separate. Channel vocabulary covers
development, Android Play/direct APK, desktop formats, iOS Store and Web delivery.
The executable metadata policy currently accepts Android direct APK. Channel
enumeration alone supplies no runtime provider.

```mermaid
flowchart LR
  Policy[Application-pinned public keys and origins] --> Verify[Authenticate exact payload]
  Feed[Bounded signed envelope] --> Verify
  Verify --> Checks[Identity version signer API ABI time and sequence]
  Checks --> Commit[Persist atomic replay checkpoint]
  Commit --> Offer[VerifiedUpdate]
  Offer --> Bytes[Bounded digest and size verification]
  Bytes --> UI[Live UI and current-offer validation]
  UI --> Native[Native installer handoff receipt]
  Native --> Probe[Fresh installed identity after relaunch]
  Probe --> Complete[Completed]
```

## Signed format and pinned trust

SignedMetadata v1 is a JSON envelope with `schema_version`, `key_id`, `payload`
and lowercase hexadecimal `signature`. Sign Ed25519 over SIGNATURE_DOMAIN
followed by the exact UTF-8 bytes in the payload string. Authenticate before
parsing that string. The verifier uses `ed25519-dalek` strict verification and
rejects weak pinned public keys. Application policy pins up to eight public keys.
Metadata cannot replace that trust policy. Private signing keys remain in the
publisher's external signing infrastructure.

The envelope is bounded to 128 KiB and payload to 32 KiB. Unknown and duplicate
fields are rejected by strict typed deserialization. UpdateMetadata contains
application ID, channel, display version, versionCode, monotonic sequence,
issued/expiry UTC seconds and one ArtifactMetadata. Artifact metadata contains
HTTPS URL, SHA-256, byte length, APK certificate SHA-256, ABI list and API range.

Validation requires the installed application ID, a strictly increasing Android
versionCode, the same installed signer, a supported ABI and API 35 or later.
Signer rotation/lineage requires a future explicit native trust policy. Digests
and signatures use exact lowercase hexadecimal encoding. Artifact size is bounded
by an application limit of at most 2 GiB. Metadata lifetime is at most seven days,
with a five-minute future clock allowance. Expired offers fail again at handoff.

URLs require HTTPS, a host, no user information, no fragment and no controls or
whitespace. Policy pins exact origins, including ports. Subdomains and alternate
ports receive no implicit trust. Download adapters must reapply this origin policy
to every redirect, retain size bounds while streaming and avoid logging URLs.

## Persistence, retry and execution ownership

Open UpdateVerifier on the application's existing AppData owner from a background
executor. It acquires an OS file lock for one trust-checkpoint writer and reuses
SettingsStore's bounded atomic replacement. Share it with Arc for multiple views.
The checkpoint binds application ID, highest sequence/versionCode, payload digest
and observed time. Corrupt, unknown-schema or inconsistent persisted data fails
closed and retains the original. No automatic reset bypasses replay protection.

The same sequence permits an idempotent retry only for a byte-identical signed
payload. Lower sequences/versions, conflicting equal sequences and significant
clock rollback fail. Commit the checkpoint before exposing VerifiedUpdate. A
newer accepted payload invalidates held older offers through revalidation.
The local checkpoint protects restart/retry ordering within private AppData.
Rollback-resistant OS storage and trusted-time acquisition require native policy
for environments whose threat model includes local storage/clock manipulation.

Signature verification, stream hashing and persistence run on a background worker
with a UI-free OperationContext. The stream verifier polls cancellation/deadline
between reads and bounds actual bytes against authenticated size. A blocking Read
implementation must supply its own I/O timeout/cooperative interruption. Its
private ArtifactVerification receipt proves the inspected stream. Native adapters
must preserve that byte identity through private-file inspection and URI grant.
Recheck the actual APK application/version/certificate and install-source
authorization before asking Android to install.

## State and receipts

UpdateController moves through Idle, Checking, UpToDate/Available, Downloading,
Verified, AwaitingUserAction, Installing and Completed, with ordinary cancellation
and explicit retryable/non-retryable failure states. Every async phase receives
an opaque session/generation ticket. Foreign/old tickets cannot update a new
attempt. Progress is monotonic and bounded by signed length. Phase-invalid
callbacks are ignored or rejected without replacing authoritative state.

Handoff requires a current owning-thread UiContext, a verified artifact receipt,
unexpired current metadata and fresh policy validation. Retain the context through
native acceptance so an expired UI owner cannot complete its old presentation.
Cancellation after handoff cannot undo native UI. Reconcile or report a correlated
native decline before retrying. An accepted handoff reports Installing.
Only a fresh installed-identity query matching the expected app, certificate and
version marks Completed. This query also reconciles a handoff whose old UI
callback was dropped after recreation. Higher unexpected versions require a new
application reconciliation policy.

## Validation and remaining adapters

Host tests cover signed/byte tampering, wrong key, strict bounds/schema/duplicates,
app/channel/version/signer/API/ABI/origin/expiry mismatches, persisted replay and
clock rollback, equal-sequence retry, competing checkpoint owners, corruption,
cancellation, stale/foreign callbacks, monotonic progress, superseded offers,
expired UI context, native decline/retry and installed-version completion.
Strict Clippy and Android ARM64/x86_64 API 35 cross-checks pass.

The initial core delivery passes 12 update tests, including key-removal
revalidation. Integrated `mise run check` passes 155 host tests plus six
compile-fail doctests and two nested child-process reports. Android API 35
ARM64/x86_64 strict Clippy passes. The facade's dependency-light `updates`
feature checks independently of native GPUI rendering features.

The next #6 slice owns atomic bounded download/cache writes, durable download and
installer-attempt recovery, redirect/origin enforcement, Android FileProvider URI
grant lifetime, install-source permission, actual APK signature/identity parsing,
native user cancellation and post-relaunch package probing. Play test-track flow,
same-signer v1-to-v2 upgrade retaining app data and interrupted channel flows are
required native acceptance. Physical-device testing remains deferred. #67 links
desktop/iOS/Web provider implementations with their independent qualification.
