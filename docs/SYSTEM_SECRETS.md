# Secure credential storage

The `secrets` facade feature exposes `gpui_box::secrets`, implemented by the
independent `gpui-secrets` package. It consumes the shared `gpui-system`
capability, error and cancellation types. A missing secure provider returns
`Unavailable`; credentials never fall back to private settings, files or logs.

## Contract and execution

Construct `Secrets::native(app_id)` or install a test/application provider with
`Secrets::with_provider`. A `CredentialKey` is an account, service and purpose.
Each component and application ID is nonempty, control-character-free and bounded
to 128 UTF-8 bytes. The namespace encodes each component separately and reserves
explicit boundaries. Encoding is reversible metadata isolation, not encryption.
Account/service names may be visible to the native credential manager. Use opaque
identifiers when those names are sensitive.

`put`, `get`, `delete` and `remove_account` are **blocking background operations**.
In GPUI, clone the service, take owned inputs, and execute using the coordinated
background executor. They reject an `OperationContext` containing UI state.
The shared registry can retain a service wrapper while background work owns a
Send service clone. Calls recheck deadline/cancellation and current provider
capabilities before execution; protection-policy validation is followed by another
check before a write. Native synchronous calls cannot be interrupted midway.
A successful write/delete reports its actual outcome even if cancellation arrives
after the native side effect; cancellation cannot undo an accepted operation.

`ServiceResult<SecretAccess<T>>` separates ordinary cancellation, typed shared
errors and the provider states `Locked` and `KeyInvalidated`. A missing credential
is `Available(None)`, which must not be interpreted as a locked provider.
Native errors contain only static sanitized messages and shared error codes.
No native exception, target, account ID or secret is appended to diagnostics.
Provider implementations are trusted code and must obey that same restriction.

`SecretBytes` has no Debug, Display, Clone or serialization implementation. The
caller must explicitly borrow its bytes, controls their lifetime, and should avoid
copying them. Owned buffers are zeroized on drop, including rejected oversized
values. Native read/enumeration secret buffers are cleared before their native
allocation is freed. This is best-effort memory clearing; it cannot erase caller
copies, OS-managed storage, crash dumps, page files or copies made by third-party
code. Records are bounded to 2,560 bytes, the Windows generic-credential limit.

## Protection policy and native behavior

`SecretPolicy` expresses required device binding, device-unlock gating, user
presence, synchronization prohibition and backup exclusion. The current Windows
provider rejects every non-default requirement **before changing a credential**.
Its implemented guarantee is the operating system's credential store for the
current Windows user with `CRED_PERSIST_LOCAL_MACHINE` persistence. That does not
prove hardware binding, absence from backups, biometric user presence, automatic
relocking on screen lock, or protected nonexportable key use. Those promises must
never be inferred from storage success. Nonexportable keys and local verification
are separate capabilities/providers under #28 and later protected-key work.

Windows uses `CredWriteW` for atomic per-target replacement, `CredReadW` and
`CredDeleteW` for retrieval/idempotent deletion, and scoped `CredEnumerateW` for
account cleanup. Targets use `gpui-box/v1/<app>/<account>/<service>/<purpose>` with
bounded hex components. Native entry points reject other target shapes and
unscoped enumeration. Enumeration applies the current app/account prefix and
validates every returned generic target again before deletion. It is capped at
4,096 records per account; larger accounts require an explicit future batching
contract. Windows `ERROR_NOT_FOUND` is ordinary absence and
`ERROR_NO_SUCH_LOGON_SESSION` is the recoverable `Locked` state. The provider does
not invent a distinct hardware-invalidated result that Windows did not report.

These names are cooperative application isolation inside one OS-user vault, not
a security boundary between processes running as that same user. The app owns
its identity. Reusing another application's identity addresses that namespace.
Changes to app/account identity require explicit read/write/delete migration;
no automatic cross-identity migration or downgrade occurs. Platform uninstall,
profile restore and backup behavior require release qualification; credentials
must not be assumed removed merely because an application is uninstalled.

Concurrent puts to one target use Windows' native replacement; the last completed
write wins. `remove_account` is not a transaction: stop issuing writes for the
account and drain existing operations before logout cleanup. It returns the count
already deleted and a completion/cancellation/locked/invalidated/failure status.
Partial failure can be retried idempotently. Other accounts, services outside the
selected account, and applications with a distinct app ID are not deleted. There
is no global erase-vault API.

## Native boundary decision

Only `src/windows.rs` contains unsafe code. The package uses `deny(unsafe_code)`
and allows that single module because Windows credential allocations require
FFI. Pointer lifetimes, bounds, native ownership and buffer clearing are described
at each unsafe use. This narrow exception leaves the portable model/service and
consumer APIs safe and leaves workspace lint policy unchanged.

## Verification and remaining platform work

Windows x86_64 evidence was collected on 2026-11-11 with rustc 1.98.1. Native tests
create a unique scratch app namespace, create/read/update/reopen a credential,
read it from a fresh process, verify adjacent-account isolation, remove the chosen
account, perform idempotent deletion and clean up through a panic-safe guard. Only
these scratch records are read/deleted; tests never list unrelated credentials or
print secret bytes. Deterministic provider tests cover denial, locked and
invalidated states, missing providers, unsupported policy, cancellation during
policy validation, partial account cleanup, malformed enumeration and input bounds.
Compile-fail doctests ensure implicit Debug/Clone access is absent.

| Platform | Provider | Implementation / evidence |
| --- | --- | --- |
| Windows | Credential Manager, current OS user | Native scratch credential and subprocess tests passed. Package identity, OS lock/backup/uninstall scenarios need #4/#5/#8 qualification. |
| Android | None installed | Portable facade/core cross-compilation only. Keystore-backed encryption, ciphertext lifecycle, invalidation and backup policy remain #27 with host #2. No plaintext fallback. |
| macOS | None installed | Contract only; Keychain access classes, session lock and native runtime tests remain #8/#27. |
| Linux | None installed | Contract only; Secret Service discovery, locked session and native runtime tests remain #8/#27. |
| iOS | None installed | Contract only; Keychain access groups/protection/backup and device evidence remain #9/#27. |

Per-operation evidence is conservative `Compiled` for the installed Windows
provider and `ContractOnly`/unavailable for the missing-provider adapter. The
matrix, rather than a successful compile, records the actual runtime scenarios.
Issue #27 remains open for the platform adapters and runtime gates above.

```sh
cargo +1.98.1 test --locked -p gpui-secrets
cargo +1.98.1 clippy --locked -p gpui-secrets --all-targets -- -D warnings
cargo +1.98.1 check --locked -p gpui-box --features secrets
cargo +1.98.1 check --locked -p gpui-box --features secrets --target aarch64-linux-android
cargo +1.98.1 check --locked -p gpui-box --features secrets --target x86_64-linux-android
```
