# System proxy, certificate, connectivity and scoped revalidation policy

This document specifies `crates/gpui-network` for
[#69](https://github.com/jihuayu/gpui-box/issues/69). The crate builds on the
typed system contracts from
[#19](https://github.com/jihuayu/gpui-box/issues/19) (`gpui-system`), the host
lifecycle snapshots from
[#21](https://github.com/jihuayu/gpui-box/issues/21) (`gpui-lifecycle`), the
background-only credential boundary from
[#27](https://github.com/jihuayu/gpui-box/issues/27) (`gpui-secrets`) and the
screen-owned query cache from
[#3](https://github.com/jihuayu/gpui-box/issues/3)
(`gpui_box_components::query::QueryClient`). It coordinates revalidation and
bounded retries with
[#43](https://github.com/jihuayu/gpui-box/issues/43) without claiming a native
provider that is not installed.

The default build has no GPUI dependency. It installs no native OS provider,
does not synthesize connectivity or trust, and never claims runtime
qualification. Every cell in the platform matrix is
`Evidence::ContractOnly`.

## Scope and ownership

`gpui-network` owns:

- Six network operations with a six-platform, thirty-six-cell qualification
  matrix and a browser capability policy.
- Strict, credential-free `EndpointOrigin` validation and normalized proxy
  endpoint parsing.
- Layered interface reachability, captive-portal, cost/data-saver, DNS, proxy,
  TLS and endpoint-health states that cannot be conflated.
- Validated `Routable` interface invariants and explicit observation
  provenance.
- Ordered system proxy resolution: direct, static rules, PAC metadata, dynamic
  native resolution and browser-opaque policy.
- Deterministic bypass matching for loopback, IPv4/IPv6 CIDR, domain suffix,
  exact host and exact host/port.
- A `gpui-secrets`-backed proxy authentication hook with background-only reads,
  zeroizing redacted headers and a bounded two-attempt budget per policy epoch.
- A trust-store policy with SHA-256 SPKI pins, fail-closed Web/native platform
  validation and deterministic certificate-chain evaluation.
- Explicit client ownership leases (`Application`, `Surface`, `Account`) with
  account fingerprints that never expose the raw account identifier.
- Monotonic policy epochs, minimal semantic diffs, scoped revalidation
  directives and a bounded per-client directive queue.
- Lifecycle-aware deferral and coalescing of stale-query revalidation while
  backgrounded or suspended.
- A deterministic integer retry controller with debounce, cooldown and
  epoch reset.
- A bounded `EventSubscription<NetworkEvent>` stream using
  `gpui_system::event_channel` (`DEFAULT_NETWORK_EVENT_CAPACITY = 64`).
- An optional GPUI adapter with an `App` global, `SystemServices` registration,
  `NetworkSurfaceScope` and `NetworkEntityModel`.

It does **not** own:

- Native Windows WinHTTP/NLM/CryptoAPI clients, macOS Network.framework/
  CFNetwork/Security.framework clients, Linux NetworkManager/portal clients,
  Android `ConnectivityManager`/Network Security Config bridges, iOS
  `NWPathMonitor`/ATS bridges or browser JavaScript hosts.
- Packaging configuration such as Android `network_security_config.xml`, Apple
  `NSAppTransportSecurity` keys, Linux CA bundles inside a sandbox or a browser
  proxy policy. Missing configuration surfaces as a typed error, never as a
  synthetic success.
- The actual HTTP stack, connection pools or TLS handshake. Consumers apply
  `RevalidationDirective`s to their own pools.
- Credential storage. Values remain in `gpui-secrets`; this crate only borrows
  them through an explicit source and redacts the resulting header.
- Remote telemetry, query strings, SSIDs, BSSIDs/MAC addresses, account names or
  certificate payload logging.

`gpui-box` exposes the crate through two opt-in features:

```toml
gpui-box = { features = ["network"] }
gpui-box = { features = ["network-gpui"] }
```

`network` imports `gpui_box::network` and enables the system, lifecycle and
secrets dependencies. `network-gpui` additionally compiles the optional GPUI
adapter and the shared system/lifecycle GPUI integrations. Neither feature
selects a desktop or Android backend by itself. `mise.toml` includes
`-p gpui-network` in `tasks.test`, `tasks.lint` and both cargo commands in
`tasks.check`, while preserving `tasks."check:templates"`.

## Architecture

```mermaid
flowchart LR
    Host[Host / UI thread] --> Service[NetworkService]
    Lifecycle[AppPhase / LifecycleState] --> Service
    Context[OperationContext] --> Gate
    Service --> Gate[Context + capability + platform gate]
    Gate --> Policy[Proxy / trust / workload policy]
    Gate --> Provider[NetworkProvider]
    Provider --> Fake[FakeNetworkProvider]
    Provider --> Unavailable[UnavailableNetworkProvider]
    Provider --> Native[Native provider, later issue]
    Secrets[gpui-secrets] --> Auth[Proxy auth hook]
    Auth --> Redacted[ProxyAuthHeader / [REDACTED]]
    Service --> Clients[NetworkClientLease registry]
    Clients --> Directives[ScopedRevalidation queue]
    Service --> Stream[Bounded NetworkEvent stream]
    Service --> Retry[RetryTracker]
    Adapter[Optional GPUI adapter] --> Service
    Adapter --> Global[App global / SystemServices]
    Adapter --> Scope[NetworkSurfaceScope]
    Adapter --> Entity[NetworkEntityModel]
    Entity --> Scope
    Scope --> Clients
```

`NetworkService` is a clonable `Send + Sync + 'static` handle over
`Arc<ServiceInner>`. It can be registered as a concrete typed service in
`gpui_system::SystemServices`, installed as a GPUI `App` global, or used
headlessly. Clones share the cached snapshot, proxy/trust policy, policy epoch,
client registry, observers, subscriber list and retry tracker;
`shares_state_with` compares the inner `Arc`.

The provider boundary is authoritative:

```rust
pub trait NetworkProvider: Send + Sync + 'static {
    fn capability(&self, operation: NetworkOperation) -> Capability<NetworkOperation>;
    fn snapshot(&self) -> Result<NetworkSnapshot, SystemError>;
    fn resolve_proxies(
        &self,
        target: &EndpointOrigin,
        config: &ProxyConfig,
    ) -> Result<Vec<ProxyCandidate>, SystemError>;
    fn diagnose_endpoint(&self, target: &EndpointOrigin)
        -> Result<EndpointDiagnostic, SystemError>;
    fn attach_listener(&self, listener: Weak<dyn NetworkProviderListener>);
    fn as_any(&self) -> &dyn Any;
}
```

Providers notify the service synchronously through a weak listener, never while
holding their own state lock. The service rechecks the execution context,
platform policy and capability at execution time rather than trusting a cached
snapshot.

### Lock discipline

Service state, client state and the credential-source slot are separate
mutexes. The service never holds an internal state lock while:

- calling `NetworkProvider` methods or `Secrets::get`;
- invoking a revalidation observer callback;
- calling a credential source;
- releasing or revoking a client lease.

Callbacks may therefore call `current_snapshot`, `resolve_proxies`,
`evaluate_workload_budget`, `register_client` or any other service method
without deadlocking. The reentrant callback integration test proves this
explicitly. No destructor panics on a poisoned lock; all lock helpers recover
the guard.

## Strict endpoint origins

`EndpointOrigin` is a scheme, a normalized host and a non-zero port. It is the
only URL shape accepted by proxy rules, PAC metadata, captive-portal origins and
endpoint diagnostics.

| Component | Accepted values |
| --- | --- |
| `OriginScheme` | `Http` (80), `Https` (443), `Ws` (80), `Wss` (443) |
| Host | lowercase ASCII DNS name, IPv4 literal, bracketed IPv6 literal |
| Port | non-zero `u16`; the scheme default applies when omitted |

Validation is deliberately strict:

- Paths (`/foo`), trailing slashes, query strings (`?a=b`), fragments
  (`#frag`) and `user:pass@` userinfo fail with `ErrorCode::InvalidInput`.
- Control characters, whitespace, backslashes, percent escapes and unbracketed
  IPv6 literals are rejected.
- DNS labels must contain 1–63 ASCII alphanumerics or hyphens, start and end
  with an alphanumeric, and the whole host is bounded to 253 bytes. Trailing
  dots and empty labels are rejected.
- Ports must be decimal and non-zero; `:0` is rejected.
- IPv4 is parsed with `Ipv4Addr`; IPv6 is parsed with `Ipv6Addr` and
  canonicalized inside brackets.

`EndpointOrigin::sanitize_url_origin` accepts an absolute URL and strips its
path, query and fragment, but rejects userinfo rather than stripping it. This is
the helper used by `CaptivePortalStatus::detect_from_url`, so a portal login URL
like `https://portal.example/login?token=...` stores only
`https://portal.example`. Query tokens, session identifiers, Wi-Fi SSIDs,
BSSIDs/MAC addresses and local filesystem paths are never retained by the
captive-portal type.

## Layered connectivity states

Link reachability is independent of endpoint availability.

### Interface / link

`InterfaceReachability` has three states:

| State | Meaning |
| --- | --- |
| `Offline` | No usable interface is reported. |
| `LocalLinkOnly` | A link exists but no routable address family is available. |
| `Routable { primary_transport, is_vpn, ipv4, ipv6 }` | At least one address family is routable. |

A `Routable` interface with both `ipv4 == false` and `ipv6 == false` is invalid.
`InterfaceReachability::routable`, `NetworkSnapshot::new`,
`NetworkSnapshot::routable` and `NetworkSnapshot::validate` reject it with
`ErrorCode::InvalidInput`. `evaluate_workload_budget` also fails closed if it is
constructed directly with that invalid combination.

`TransportKind` is `WiFi`, `Cellular`, `Ethernet`, `VirtualTunnel`, `Loopback`
or `Other`. `SignalSource` records explicit provenance: `Observed`, `Inferred`,
`Unavailable` or `Unknown`. Only `Observed` reports `is_observed() == true`.

### Captive portal

`CaptivePortalStatus` is `Unknown`, `Clear`, `Suspected` or
`Detected { portal_origin: Option<EndpointOrigin> }`. A detected portal stores
only a validated origin. `CaptivePortalStatus::detect_from_url` sanitizes an
absolute URL and fails closed on userinfo.

A detected portal blocks every workload class. A suspected portal permits
`Interactive` so a foreground probe or login check can proceed, but blocks
`Revalidation`, `Prefetch` and `BackgroundBulk`. `Unknown` is not treated as
`Clear` for health claims, but on a routable interface it does not block
workloads that are otherwise permitted by cost policy.

### Cost and data saver

`ConnectionCost` is `Unmetered`, `Metered` or `CellularRoaming`.
`DataSaverMode` is `Off`, `Constrained` or `Unavailable`.
`NetworkWorkloadClass` is `Interactive`, `Revalidation`, `Prefetch` or
`BackgroundBulk`.

The deterministic policy is:

| Condition | Interactive | Revalidation | Prefetch | BackgroundBulk |
| --- | --- | --- | --- | --- |
| `Offline`, `LocalLinkOnly`, captive `Detected` | Block | Block | Block | Block |
| Captive `Suspected` | Allow | Block | Block | Block |
| `DataSaverMode::Constrained` or `CellularRoaming` | Allow | Defer | Defer | Defer |
| `Metered` | Allow | Allow | Defer | Defer |
| `Unmetered` with saver `Off`/`Unavailable` | Allow | Allow | Allow | Allow |

`Block` is `WorkloadBudgetDecision::BlockWhileOfflineOrCaptive`, `Defer` is
`WorkloadBudgetDecision::DeferUntilUnmetered`. The method is available on both
`NetworkSnapshot` and `NetworkService`; the service evaluates its cached
snapshot. No wall clock or provider call participates.

### Endpoint diagnostics

`EndpointDiagnostic` keeps DNS, proxy, TLS and endpoint outcomes distinct:

```text
Offline
CaptivePortalBlocked
ProxyResolutionFailed
ProxyUnreachable
ProxyAuthRequired { realm }
ProxyAuthExhausted
DnsFailure { kind: NonExistentDomain | Timeout | ServerFailure | Refused | NoRecords }
TcpConnectTimeout
TlsVerificationFailed { reason: UntrustedRoot | HostnameMismatch |
                                 ExpiredOrNotYetValid | Revoked | PinMismatch }
EndpointReachable { latency_ms }
EndpointHttpUnavailable { status_code }
```

A proxy authentication realm is bounded to 128 bytes. Control characters and
over-length realms are rejected; an empty or whitespace-only realm becomes
`None`. No credential value can appear in a realm constructor.

## System proxy policy

`ProxyConfig` variants:

- `Direct`.
- `StaticRules { proxies: Vec<ProxyRule>, bypass: Vec<BypassPattern> }`.
- `PacUrl { script_origin: EndpointOrigin, script_id: String }`.
- `SystemDynamic`.
- `BrowserOpaque` — the mandatory representation on Web.

`ProxyRule` pairs a `ProxyScheme` (`Http`, `Https`, `Socks5`) with a validated
proxy `EndpointOrigin` and an optional target `OriginScheme` filter. Rules are
evaluated in vector order, so an application can express ordered HTTP, HTTPS and
SOCKS5 fallback without map iteration nondeterminism.

`ProxyCandidate` is `Direct`, `Http { proxy }`, `Https { proxy }` or
`Socks5 { proxy }`.

### Bypass matching

| Pattern | Matching rule |
| --- | --- |
| `Loopback` | `localhost`, `*.localhost`, `127.0.0.0/8`, `::1` |
| `Ipv4Cidr { network, prefix_len }` | `prefix_len <= 32`; pure `u32` mask arithmetic |
| `Ipv6Cidr { network, prefix_len }` | `prefix_len <= 128`; pure `u128` mask arithmetic |
| `DomainSuffix(String)` | normalized suffix; `*.corp.example` matches `api.corp.example` and `a.b.corp.example`, never `badcorp.example` |
| `ExactHost(String)` | normalized lowercase host, IPv4 or bracketed IPv6 |
| `ExactHostPort { host, port }` | exact host and non-zero port |

Prefix zero is handled without shifting by the full integer width, so
`0.0.0.0/0` and `::/0` are safe. Malformed prefixes constructed directly are
reported as non-matching and rejected by validation. A direct IPv4 or IPv6
bypass never matches the other family.

`ProxyConfig::resolve_proxies` returns `vec![Direct]` when any bypass matches,
when no rule matches the target scheme, or for direct/PAC/dynamic/browser
variants at the pure policy layer. `NetworkService::resolve_proxies` handles the
platform boundary:

1. Validate the execution context and cancellation.
2. Validate the proxy representation for the configured platform.
3. Fail closed with `ErrorCode::Unsupported` on Web or for `BrowserOpaque`.
4. Check the `ResolveSystemProxy` capability.
5. Resolve `Direct`/`StaticRules` deterministically, or delegate
   `SystemDynamic`/`PacUrl` to the provider.
6. Recheck the deadline and cancellation after the external call before
   returning candidates.

`PacUrl::script_id` is an opaque ASCII identifier of 1–128 characters using only
alphanumerics, dots, underscores and hyphens. Query characters, paths, spaces,
control characters and over-length identifiers are rejected. A PAC origin is a
validated `EndpointOrigin`, so a PAC URL query token cannot be retained.

On Web, `BrowserOpaque` is the only accepted representation. On native targets,
`BrowserOpaque` is rejected because no browser owns the proxy. This is checked
both at service construction and at `set_proxy_config`.

### Host proxy invalidation

Native providers can report `NetworkProviderNotice::ProxyConfigurationInvalidated`
when the OS proxy, PAC result, bypass list or VPN split tunnel changes.
`NetworkService::invalidate_proxy_configuration()` applies the same transition
directly, and `FakeNetworkProvider::invalidate_proxy_configuration()` emits the
notice deterministically for tests.

The behavior depends on the configured `ProxyConfig`:

| Current config | Epoch | Events | Directives |
| --- | --- | --- | --- |
| `SystemDynamic` | advance | `ProxyConfigChanged` + `PolicyEpochAdvanced` | `DropAllPoolsAndReconnect`; plus `RevalidateStaleQueries` when routable and not captive-blocked |
| `PacUrl { .. }` | advance | `ProxyConfigChanged` + `PolicyEpochAdvanced` | same as `SystemDynamic` |
| `Direct` | unchanged | none | none |
| `StaticRules { .. }` | unchanged | none | none |
| `BrowserOpaque` | unchanged | none | none |

Dynamic invalidation clears the proxy-authentication attempt budget through
`advance_epoch_locked`, syncs the retry controller to the new epoch, and
dispatches directives to every live client and revalidation observer. When the
app is foreground-active it also flushes any pending stale-query revalidation;
while backgrounded or suspended, the normal lifecycle deferral still applies.

Explicit direct, static and browser-opaque policies are deliberately a no-op:
an OS proxy change does not alter an application-owned route or a browser-owned
proxy. In that case the return value is `PolicyUpdate::unchanged(epoch)` and no
`PolicyEpochAdvanced`, `ProxyConfigChanged` or reconnect directive is emitted.
`BrowserOpaque` is only valid on Web, where the browser owns all proxy state.

`FakeNetworkProvider::rotate_proxy_credentials()` emits
`NetworkProviderNotice::ProxyCredentialsRotated`, exercising the coordinated
epoch advance plus `ReauthenticateProxy` and `RecycleIdleConnections` delivery.

## Proxy authentication and `gpui-secrets`

`ProxyAuthScheme` is `Basic` or `Bearer`. `ProxyAuthChallenge` carries a
`ClientId`, a validated proxy `EndpointOrigin`, a `PolicyEpoch`, the scheme and
an optional sanitized realm.

`ProxyCredentialSource` is the explicit credential-read boundary:

```rust
pub trait ProxyCredentialSource: Send + Sync + 'static {
    fn credential(
        &self,
        challenge: &ProxyAuthChallenge,
        context: &OperationContext,
    ) -> ServiceResult<SecretAccess<Option<SecretBytes>>>;
}
```

`SecretsProxyCredentialSource` wraps `gpui_secrets::Secrets` and a
`CredentialKey` and delegates directly to `Secrets::get`. Reading proxy
credentials is background-only: a context with `ui.is_some()` returns
`ErrorCode::InvalidInput`, matching `gpui-secrets`. Expired deadlines return
`TimedOut`; cancellation returns `Completion::Cancelled`.

`ProxyAuthHeader` wraps `SecretBytes` (zeroized on drop, no `Clone`, no `Debug`,
no serialization) plus scheme metadata. Its `Debug` output contains
`"[REDACTED]"`; the only payload access is
`expose_header_value(&self) -> &[u8]`. `ProxyAuthOutcome` is `Header`,
`Exhausted`, `Locked`, `KeyInvalidated` or `MissingCredentials`, and its `Debug`
prints only the public-safe kind.

### Bounded challenge budget

`MAX_PROXY_AUTH_ATTEMPTS = 2`. Attempts are tracked per
`(ClientId, proxy EndpointOrigin, PolicyEpoch)`.

- The first and second credential-bearing 407 challenges return
  `ProxyAuthOutcome::Header`.
- Once two attempts have been recorded, the next challenge returns
  `ProxyAuthOutcome::Exhausted` without re-reading secrets or re-sending stale
  credentials.
- A different proxy origin or a new policy epoch has an independent budget.
- `SecretAccess::Locked` maps to `Locked`, `KeyInvalidated` maps to
  `KeyInvalidated`, `Available(None)` maps to `MissingCredentials`, and an
  exhausted budget maps to `Exhausted`.
- `rotate_proxy_credentials(scope)` and
  `notify_proxy_credentials_rotated(scope)` advance the policy epoch, reset the
  relevant attempt budget and emit `ReauthenticateProxy` plus
  `RecycleIdleConnections` only to live clients matching the scope. A global
  rotation (`None`) targets every live client.
- Any policy epoch advance clears proxy authentication attempts.

## Trust-store and certificate policy

`SpkiPinSha256` is a public-safe SHA-256 SPKI pin. It supports `from_bytes`,
`from_hex`, `to_hex`, ordering, equality, serde and a safe `Debug`/`Display`
(the hex digest). It never contains a certificate body or private key.

`TrustStorePolicy` variants:

- `OsSystemRoots`.
- `OsPlusEnterpriseRoots { allow_user_installed_roots }`.
- `StrictSpkiPinSet { pins, include_system_roots, allow_enterprise_bypass }`.
- `BrowserManagedTls`.

A strict pin set must be non-empty. `TrustProfile` is `Release` (default) or an
explicit `DebugOverride`.

### Fail-closed platform validation

`validate_trust_policy(platform, profile, policy)` is called when configuring
`NetworkService` and when changing policy:

| Platform | Accepted policy |
| --- | --- |
| Web | Only `BrowserManagedTls`; all custom roots and pins fail `Unsupported` |
| Windows/macOS/Linux/Android/iOS | Native roots or strict pins; `BrowserManagedTls` fails `Unsupported` |
| Android/iOS `Release` | `OsPlusEnterpriseRoots { allow_user_installed_roots: true }` fails `InvalidInput` |
| Android/iOS `DebugOverride` | User-installed roots may be selected explicitly |
| Any native target | `StrictSpkiPinSet` with empty pins fails `InvalidInput` |

A pinned Web policy therefore never silently degrades to unpinned browser TLS.

### Deterministic evaluation

`evaluate_tls_trust(chain_observation, policy)` evaluates in this fixed order:

1. Empty chain → `Failed(UntrustedRoot)`.
2. Revoked → `Failed(Revoked)`.
3. Expired or not yet valid → `Failed(ExpiredOrNotYetValid)`.
4. Hostname mismatch → `Failed(HostnameMismatch)`.
5. Anchor selection and pin matching.

`CertificateChainObservation` carries the ordered certificate observations,
hostname/revocation/validity booleans and an optional browser-verified flag.
Each `CertificateObservation` carries an `ObservedRootKind`
(`System`, `Enterprise`, `UserInstalled`, `Unknown`) and an optional SPKI pin.

Verified anchors are `SystemAnchor`, `EnterpriseAnchor`, `PinnedDirectAnchor`
and `BrowserManaged`.

| Policy | Root kind | Result |
| --- | --- | --- |
| `OsSystemRoots` | System | `SystemAnchor` |
| `OsSystemRoots` | Enterprise/UserInstalled/Unknown | `UntrustedRoot` |
| `OsPlusEnterpriseRoots` | System | `SystemAnchor` |
| `OsPlusEnterpriseRoots` | Enterprise | `EnterpriseAnchor` |
| `OsPlusEnterpriseRoots { allow_user_installed_roots: true }` | UserInstalled | `EnterpriseAnchor` |
| `OsPlusEnterpriseRoots { allow_user_installed_roots: false }` | UserInstalled | `UntrustedRoot` |
| `OsPlusEnterpriseRoots` | Unknown | `UntrustedRoot` |
| `StrictSpkiPinSet` with empty pins | any | `PinMismatch` |
| Strict, `include_system_roots: true` | System | `SystemAnchor` on pin match, else `PinMismatch` |
| Strict, `include_system_roots: true` | Enterprise, bypass allowed | `EnterpriseAnchor` even without a pin |
| Strict, `include_system_roots: true` | Enterprise, bypass denied | `EnterpriseAnchor` on pin match, else `PinMismatch` |
| Strict, `include_system_roots: true` | UserInstalled/Unknown | `UntrustedRoot` |
| Strict, `include_system_roots: false` | Enterprise with bypass | `EnterpriseAnchor` |
| Strict, `include_system_roots: false` | any root with pin match | `PinnedDirectAnchor` |
| Strict, `include_system_roots: false` | any root without pin match | `PinMismatch` |
| `BrowserManagedTls` | browser-verified nonempty chain | `BrowserManaged`; an unverified chain fails `UntrustedRoot` |

Revocation and expiry always take precedence over a matching pin or an
enterprise bypass.

## Client ownership and scoped revalidation

`ClientId(u64)` identifies a live lease. `PolicyEpoch` is monotonic and starts
at `PolicyEpoch::INITIAL` (`1`). `ClientOwnerScope` is:

- `Application`.
- `Surface { surface: SurfaceId, generation: u64 }`.
- `Account { account_key: AccountScopeKey }`.

`AccountScopeKey` validates the raw account identifier, hashes it with FNV-1a 64
and retains only the digest. `Debug`, `Display` and serde never contain the raw
string. Tests assert that the raw account value is absent from the debug output
of `AccountScopeKey`, `ClientOwnerScope`, `NetworkClientLease` and
`NetworkEvent`.

### `NetworkClientLease`

`NetworkService::register_client(scope, context)` returns a lease tracking:

- `client_id()`, `scope()`, `observed_epoch()`;
- `acknowledge_epoch(epoch)`;
- `is_active()`, `revocation_reason()`;
- `drain_directives() -> Vec<ScopedRevalidation>`;
- `release() -> bool`.

`Drop` releases an active lease automatically and is idempotent with
`release()`. The per-client directive queue is bounded to
`MAX_PENDING_DIRECTIVES = 64`.

`ClientOwnerScope::Surface` requires a non-zero `SurfaceId` and a non-zero
generation. Registration rejects `SurfaceId(0)` or generation zero with
`ErrorCode::InvalidInput` (message
`Surface client scope requires non-zero SurfaceId and generation`) before any
lease or `ClientRegistered` event is created. The service also emits
`NetworkEvent::OperationFailed` with `SubscribeRevalidation` for that rejected
attempt.

Revocation reasons are `Released`, `SurfaceDisposed`, `StaleGeneration`,
`AccountCleared` and `ServiceShutdown`.

- `dispose_surface(surface)` revokes every generation on that surface.
- `dispose_surface_generation(surface, generation)` revokes exactly one
  generation. Dropping an older `NetworkSurfaceScope` therefore cannot revoke a
  recreated newer generation.
- `revoke_stale_generations(surface, current)` revokes only other generations.
- `revoke_account_clients(&key)` revokes only matching account scopes.
- `shutdown()` revokes every live lease and rejects new registrations with
  `Unavailable`.

### Directives and semantic diffs

`RevalidationDirective` values:

```text
RecycleIdleConnections
DropAllPoolsAndReconnect
ReauthenticateProxy
RevalidateStaleQueries
ThrottleBackgroundTransfers
AwaitCaptivePortalClear
```

A `ScopedRevalidation` carries the client id, owner scope, epoch and directive.
Directives are deduplicated in policy order.

Policy transitions use semantic comparison, not struct inequality. A change to
`SignalSource` or `revision` alone never advances `PolicyEpoch` and emits zero
directives. `PolicyUpdate` reports `previous_epoch`, `epoch`, `changed` and the
global directive list.

Provider snapshots are normalized before they are cached: a provider revision
of zero or any value not greater than the cached revision becomes
`cached_revision.saturating_add(1)`.
`NetworkService::snapshot(&context)` returns that cached normalized snapshot,
not the raw provider value, so its `revision` always agrees with
`current_snapshot().revision`. Semantic equality still prevents an epoch
advance for a revision-only refresh.

| Change | Epoch | Directives |
| --- | --- | --- |
| Identical reachability/portal/cost/saver/proxy/trust | unchanged | none |
| Reachability transport/VPN/IP-family transition | advance | `DropAllPoolsAndReconnect`; plus `RevalidateStaleQueries` when the new state is routable and not captive-blocked |
| Proxy configuration change | advance | `DropAllPoolsAndReconnect`; plus `RevalidateStaleQueries` when routable and clear |
| Trust policy change | advance | `DropAllPoolsAndReconnect`; plus `RevalidateStaleQueries` when routable and clear |
| Captive portal becomes `Detected`/`Suspected` | advance | `AwaitCaptivePortalClear` + `ThrottleBackgroundTransfers` |
| Captive portal `Detected`/`Suspected` → `Clear` while routable | advance | `RecycleIdleConnections` + `RevalidateStaleQueries` |
| Cost/saver becomes constrained | advance | `ThrottleBackgroundTransfers` |
| Cost/saver relaxes while routable and clear | advance | `RevalidateStaleQueries` |
| Proxy credentials rotate | advance | `ReauthenticateProxy` + `RecycleIdleConnections` to matching live scopes only |

### Lifecycle deferral

`set_app_phase(AppPhase)` and `apply_lifecycle_state(&LifecycleState)` coordinate
with `gpui-lifecycle`.

- In `Background` or `Suspended`, `RevalidateStaleQueries` is not delivered
  immediately. Pool-hygiene directives such as `DropAllPoolsAndReconnect` and
  `ThrottleBackgroundTransfers` are still delivered, the policy epoch still
  advances, and the service plus live clients record pending stale
  revalidation.
- Returning to `ForegroundActive` while routable and not captive-blocked flushes
  **at most one coalesced `RevalidateStaleQueries` per live client**, regardless
  of how many transitions occurred while backgrounded.
- Clients registered during the background interval receive one flush on resume.
- If the network is offline, local-link-only, suspected or detected captive on
  resume, pending revalidation remains pending until policy permits it.

### Revalidation observers

`NetworkService::on_revalidation(scope_filter, callback)` returns a
`RevalidationObserverLease`. `RevalidationScopeFilter` is `All`, `Application`,
`Surface { surface }` or `Account { account_key }`; `ClientOwnerScope` and
`Option<ClientOwnerScope>` convert into it.

Callbacks receive `&ScopedRevalidation` and run outside all service, client and
observer locks. They may re-enter `current_snapshot`, `resolve_proxies`,
`register_client` or any other safe method. Dropping the observer lease
unregisters it without panicking.

## Bounded retry controller

`RetryPolicy` is plain integer data:

```rust
pub struct RetryPolicy {
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_attempts: u32,
    pub cooldown: Duration,
    pub debounce_window: Duration,
}
```

`RetryTracker` is deterministic with a caller-supplied `now: Instant`; it never
sleeps, spawns or reads the wall clock. Delays use `checked_shl` and
`saturating_mul`, are capped at `max_delay_ms`, and `execute_at` uses
`checked_add`.

`RetryDecision` is:

- `RetryAfter { attempt, delay, execute_at }` up to `max_attempts`;
- `Exhausted { cooldown_until }` once the budget is spent, until `now >=
  cooldown_until` or `reset_on_epoch_change(new_epoch)`;
- `DoNotRetry { reason }` for non-retriable failures.

Non-retriable failures short-circuit immediately:

- `TlsVerificationFailed`, `ProxyAuthExhausted`, `CaptivePortalBlocked`,
  `Offline`, `ProxyAuthRequired`;
- HTTP 4xx except `408` and `429`;
- HTTP 3xx and already-reachable outcomes.

Retriable failures: every `DnsFailure` kind, `TcpConnectTimeout`,
`ProxyResolutionFailed`, `ProxyUnreachable` and
`EndpointHttpUnavailable { 408 | 429 | 500..=599 }`.

Rapid re-evaluations inside `debounce_window` reuse the scheduled attempt instead
of spending another one. `on_network_change(now)` coalesces rapid network-change
triggers and starts a fresh budget only outside the debounce window.
`reset_on_epoch_change` resets attempts and cooldown exactly when the epoch
changes.

## Bounded event stream

Each `NetworkService::subscribe()` creates an independent bounded channel with
capacity `DEFAULT_NETWORK_EVENT_CAPACITY = 64` (or
`with_event_capacity`/`with_platform_and_event_capacity` for deterministic
tests). Producers never block on consumers.

Events:

```text
SnapshotUpdated { snapshot }
ReachabilityChanged { reachability }
CaptivePortalChanged { status }
ConnectionCostChanged { cost, data_saver }
ProxyConfigChanged { config }
TrustPolicyChanged { policy }
ProxyCredentialsRotated { scope }
PolicyEpochAdvanced { epoch }
ClientRegistered { client_id, scope }
ClientRevoked { client_id, scope, reason }
RevalidationDirective { client_id, scope, directive, epoch }
ProxyAuthHandled { client_id, proxy, outcome, epoch }
AppPhaseChanged { phase }
OperationFailed { operation, code }
```

No event contains a credential value, header payload, account string, query
token, SSID, MAC address or filesystem path.

Overflow discards the oldest queued events, announces `Event::ResyncRequired`
once and continues with the newest event. Consumers acknowledge a fresh snapshot
with `EventSubscription::acknowledge_snapshot(sequence)`. Tests publish more
than 64 events without draining and use capacity 1 to prove deterministic
eviction without sleeps or extra threads.

## Optional GPUI adapter

The optional `gpui` feature adds:

- `impl gpui::Global for NetworkService`.
- `install_network_service(service, cx)`.
- `install_network_in_services(&mut SystemServices, service)`.
- `init_network(cx, provider)`, installing both the `App` global and the
  `SystemServices` provider.
- `network_service(cx)`, checking the `App` global first and falling back to
  `SystemServices`.
- `NetworkAppExt` for `gpui::App`.
- `NetworkSurfaceScope`.
- `NetworkEntityModel`.

`NetworkSurfaceScope` captures the owner's current `SurfaceId` and generation,
retains `EventSubscription<NetworkEvent>` streams, and can register exactly one
surface-generation `NetworkClientLease`. On drop it:

1. Drops retained subscriptions.
2. Calls `dispose_surface_generation(surface, generation)` — exact generation
   only.
3. Drops its client lease.
4. Disposes the `UiOwner` only when its own `UiContext` is still current.
5. Drops the optional `EntityScope`.

The generation check is a security invariant: dropping generation 1 after
generation 2 has been created on the same `SurfaceId` revokes only generation 1.
The GPUI unit test proves that generation 2's lease remains active and only its
own drop disposes the owner.

`NetworkEntityModel` is created with
`cx.new(|cx| NetworkEntityModel::new(owner, service, cx))`. It retains the
surface scope, client lease, one bounded subscription and at most
`MAX_RETAINED_EVENTS = DEFAULT_NETWORK_EVENT_CAPACITY` (64) recent events. A
provider overflow or local history overflow sets a sticky `resync_required`
flag. On drop the lease and surface generation are released. The model exposes
`drain_events`, `retained_events`, `take_retained_events`,
`drain_client_directives`, `acknowledge_epoch`, `refresh_snapshot` and
`set_app_phase`.

## Six-platform × six-operation qualification matrix

`PlatformTarget` has six values and `NetworkOperation` has six. The complete
matrix contains exactly 36 rows, one per pair. All rows are
`Evidence::ContractOnly`: no native provider is installed by this crate, so no
row claims `Compiled` or `RuntimeVerified` qualification.

`Support` includes `Native`, `Conditional`, `FallbackAvailable`, `Unsupported`
and `Unknown`. `Conditional` marks a coarse browser-defined subset whose
availability depends on host policy; it is not equivalent to a native
observation and must not be upgraded into one.

| Platform | Operation | Support | Required host configuration | Evidence |
| --- | --- | --- | --- | --- |
| Windows | `ObserveConnectivity` | Native | `INetworkListManager.GetConnectivity` / `NLM_CONNECTIVITY`; `Windows.Networking.Connectivity.NetworkInformation.GetInternetConnectionProfile` for `NetworkCostType` | ContractOnly |
| Windows | `ResolveSystemProxy` | Native | `WinHttpGetIEProxyConfigForCurrentUser`; `WinHttpGetProxyForUrl` (WinHTTP AutoDetect/AutoConfigURL) | ContractOnly |
| Windows | `HandleProxyAuth` | Native | WinHTTP/WinINet 407 handling; `Proxy-Authenticate` realm; `gpui-secrets`; bounded attempts | ContractOnly |
| Windows | `EvaluateTrustPolicy` | Native | `CertGetCertificateChain` + `CERT_CHAIN_POLICY_SSL`; system and Group Policy enterprise roots | ContractOnly |
| Windows | `DiagnoseEndpointPath` | Native | `GetAddrInfoEx`; `ConnectEx` deadlines; WinHTTP tunnel diagnostics | ContractOnly |
| Windows | `SubscribeRevalidation` | Native | Network List Manager COM connection/media/cost events on a message-pumping thread | ContractOnly |
| macOS | `ObserveConnectivity` | Native | `NWPathMonitor`; `NWPath.isExpensive`; `NWPath.isConstrained`; `NWInterface.InterfaceType` | ContractOnly |
| macOS | `ResolveSystemProxy` | Native | `CFNetworkCopyProxiesForURL`; `SCDynamicStoreCopyProxies` | ContractOnly |
| macOS | `HandleProxyAuth` | Native | CFNetwork/URLSession 407; bounded realm; Keychain-backed `gpui-secrets` | ContractOnly |
| macOS | `EvaluateTrustPolicy` | Native | `SecTrustEvaluateWithError`; `SecTrustCopyAnchorCertificates`; admin-keychain enterprise anchors | ContractOnly |
| macOS | `DiagnoseEndpointPath` | Native | `NWConnection`/`CFHost` deadlines; `SecTrust`; CFNetwork HTTP status | ContractOnly |
| macOS | `SubscribeRevalidation` | Native | `NWPathMonitor` update handler; SystemConfiguration change notifications | ContractOnly |
| Linux | `ObserveConnectivity` | Native | NetworkManager D-Bus `org.freedesktop.NetworkManager` Connectivity/Metered; `org.freedesktop.portal.NetworkMonitor` | ContractOnly |
| Linux | `ResolveSystemProxy` | Native | `org.freedesktop.portal.ProxyResolver` / NetworkManager proxy; `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY` fallback | ContractOnly |
| Linux | `HandleProxyAuth` | Native | HTTP stack 407; Secret Service via `gpui-secrets`; per-epoch attempt cap | ContractOnly |
| Linux | `EvaluateTrustPolicy` | Native | OpenSSL/GnuTLS system trust from `/etc/ssl/certs` or `SSL_CERT_FILE`; explicit enterprise anchors | ContractOnly |
| Linux | `DiagnoseEndpointPath` | Native | `getaddrinfo`/`res_query` deadlines; non-blocking connect timeouts; TLS verification | ContractOnly |
| Linux | `SubscribeRevalidation` | Native | NetworkManager and xdg-desktop-portal NetworkMonitor D-Bus signals | ContractOnly |
| Android | `ObserveConnectivity` | Native | `ConnectivityManager.NetworkCallback`; `NET_CAPABILITY_INTERNET`, `NET_CAPABILITY_VALIDATED`, `NET_CAPABILITY_CAPTIVE_PORTAL`, `NET_CAPABILITY_NOT_METERED`, `NET_CAPABILITY_NOT_VPN`; `getRestrictBackgroundStatus()` | ContractOnly |
| Android | `ResolveSystemProxy` | Native | `LinkProperties.getHttpProxy()` / `ProxyInfo` per active `Network` | ContractOnly |
| Android | `HandleProxyAuth` | Native | `ProxyInfo` authentication; Keystore-backed `gpui-secrets`; two-attempt cap | ContractOnly |
| Android | `EvaluateTrustPolicy` | Conditional | Android Network Security Config (`network_security_config.xml`); user-CA exclusion in release builds by default; explicit `DebugOverride` for user roots | ContractOnly |
| Android | `DiagnoseEndpointPath` | Native | `Network.getAllByName`/`InetAddress`; `Socket` deadlines; platform TLS; `HttpURLConnection` status | ContractOnly |
| Android | `SubscribeRevalidation` | Native | Retained `ConnectivityManager.NetworkCallback` registration; unregister on surface disposal | ContractOnly |
| iOS | `ObserveConnectivity` | Native | `NWPathMonitor`; `NWPath.isExpensive`; `NWPath.isConstrained`; `NWPath.usesInterfaceType` | ContractOnly |
| iOS | `ResolveSystemProxy` | Native | `CFNetworkCopySystemProxySettings` | ContractOnly |
| iOS | `HandleProxyAuth` | Native | URLSession 407; Keychain-backed `gpui-secrets`; per-epoch cap | ContractOnly |
| iOS | `EvaluateTrustPolicy` | Conditional | App Transport Security (`NSAppTransportSecurity`); `SecTrustEvaluateWithError`; release user-anchor restrictions | ContractOnly |
| iOS | `DiagnoseEndpointPath` | Native | `NWConnection`/`CFHost` deadlines; URLSession task metrics; ATS cleartext blocks | ContractOnly |
| iOS | `SubscribeRevalidation` | Native | `NWPathMonitor` plus app scene lifecycle; one flush on active | ContractOnly |
| Web | `ObserveConnectivity` | Conditional | `navigator.onLine` + `online`/`offline`; coarse hint only; no interface/VPN/captive/metered detail | ContractOnly |
| Web | `ResolveSystemProxy` | Unsupported | Browser owns proxy/PAC/bypass; only `ProxyConfig::BrowserOpaque` is accepted | ContractOnly |
| Web | `HandleProxyAuth` | Unsupported | Browser owns 407, `Proxy-Authorization` and proxy credentials | ContractOnly |
| Web | `EvaluateTrustPolicy` | Unsupported | Browser owns roots, pins and certificate errors; only `BrowserManagedTls` is accepted | ContractOnly |
| Web | `DiagnoseEndpointPath` | Conditional | `fetch`/WebSocket observe application-level success only; DNS/TCP/TLS/proxy remain opaque | ContractOnly |
| Web | `SubscribeRevalidation` | Native | `online`/`offline` wake a scoped revalidation loop; consumers coalesce and re-check `navigator.onLine` | ContractOnly |

### Browser capability policy

`web_capability_policy(WebBackendMaturity)` returns a `WebNetworkPolicy`:

- `backend_name = "navigator.onLine"`.
- `connectivity_event = "online/offline"`.
- `proxy_owner = "browser"`.
- `observe_connectivity = Support::Conditional`.
- `resolve_system_proxy = HandleProxyAuth = EvaluateTrustPolicy = Unsupported`.
- `diagnose_endpoint_path = Support::Conditional`.
- `subscribe_revalidation = Support::Native`.
- `custom_tls_pins_available = false`.
- `browser_managed_tls_only = true`.
- `online_hint_is_coarse = true`.

`WebBackendMaturity` is `Experimental`, `Preview` or `Stable`; the support
contract does not change with maturity. A browser host must not upgrade a
`Conditional` observation into a native claim.

### Qualification links

| Issue | Ownership |
| --- | --- |
| [#2](https://github.com/jihuayu/gpui-box/issues/2) | Android host, `ConnectivityManager`/JNI bridge and network security configuration |
| [#3](https://github.com/jihuayu/gpui-box/issues/3) | `QueryClient`, cache invalidation and account partitioning |
| [#8](https://github.com/jihuayu/gpui-box/issues/8) | Windows/macOS/Linux native network provider qualification |
| [#9](https://github.com/jihuayu/gpui-box/issues/9) | iOS host, `NWPathMonitor`/ATS and scene qualification |
| [#18](https://github.com/jihuayu/gpui-box/issues/18) | Phase 1 sequencing and integration gate |
| [#19](https://github.com/jihuayu/gpui-box/issues/19) | Shared system-service scopes, capabilities, cancellation and error vocabulary |
| [#21](https://github.com/jihuayu/gpui-box/issues/21) | Lifecycle phases, surface generations and background/resume coordination |
| [#27](https://github.com/jihuayu/gpui-box/issues/27) | `gpui-secrets`, background-only credential reads and zeroizing values |
| [#43](https://github.com/jihuayu/gpui-box/issues/43) | Background execution, durable work and cancellation policy |
| [#54](https://github.com/jihuayu/gpui-box/issues/54) | Revalidation coordination and desktop integration |
| [#69](https://github.com/jihuayu/gpui-box/issues/69) | This crate's contract and acceptance |

## Determinism, privacy and portability

- The workspace's forbidden-unsafe-code lint applies to every module.
- No blocking sleeps in source or tests. Lifecycle and retry tests pass
  explicit `Instant` values and use channels/atomic counters for synchronization.
- No debug-print or unimplemented macros.
- No wall-clock or floating-point backoff arithmetic.
- No raw account identifier, credential value, header payload, query token,
  SSID, BSSID/MAC address or filesystem path in a public type, debug output or
  event.
- Persistent structures are bounded: 64 events, 64 pending client directives,
  128-byte credential identifiers/realms, 253-byte hostnames, 64-character hex
  pins, 32/128-bit CIDR prefixes.
- Callback and external-provider calls always occur outside internal locks, so
  reentrant callbacks cannot deadlock.
- `Drop` implementations recover poisoned locks and never panic.

## Verification

`mise.toml` pins Rust **1.98.1**. The validation host observed
`rustc 1.99.1` and `cargo 1.99.1`. Re-run under the pinned selector before
publishing packaged runtime evidence.

| Tool/source | Version or identity |
| --- | --- |
| Rust toolchain pin (`mise.toml`) | `1.98.1` |
| Validation host compiler observed | `rustc 1.99.1` / `cargo 1.99.1` |
| Coordinated GPUI revision | `f8cfc7ed9e57503def9479138894f3139f5400cc` |
| `gpui-network` package version | `0.1.0-dev` |
| serde | `1.0` |
| Formatting | `cargo fmt --all -- --check` |
| Task validation | `mise tasks validate` |

Run:

```sh
cargo fmt --all
cargo fmt --all -- --check
mise tasks validate
cargo test --locked -p gpui-network
cargo test --locked -p gpui-network --all-features
cargo test --locked -p gpui-box --features network,network-gpui --test network --lib
cargo clippy --locked -p gpui-network -p gpui-box --all-features --all-targets -- -D warnings
git diff --check
git status -sb
```

### Test coverage

The suite is deterministic and does not assume a native provider, a network
connection, OS-specific paths or wall-clock behavior.

| File | Coverage |
| --- | --- |
| `tests/model.rs` | Strict origin parsing/rejection, default ports, IPv4/IPv6 normalization, routable invariant, captive-portal sanitization, workload matrix, serde, diagnostic realm bounds |
| `tests/proxy.rs` | Loopback/CIDR/domain-suffix/exact-host bypass matching, prefix zero and boundaries, ordered candidates, target scheme filters, PAC id validation, static vs dynamic service resolution, Web fail-closed, dynamic proxy invalidation/static no-op, provider notice helpers, cancellation/deadline gates |
| `tests/trust.rs` | Pin hex/order/serde/debug, Web/native/mobile platform validation, empty pin sets, failure precedence, system/enterprise/user roots, enterprise bypass on/off, pin-only mode, browser-managed TLS |
| `tests/auth.rs` | Secrets-backed source, background-only enforcement, two-attempt budget and third-attempt exhaustion, dynamic invalidation budget reset, locked/invalidated/missing outcomes, redacted headers, realm bounds, scoped credential rotation, expired/cancelled contexts |
| `tests/lease.rs` | Register/ack/release/drop idempotence, non-zero surface scope validation, exact surface-generation disposal, stale generation revocation, account redaction and revocation, shutdown, reentrant observers, background coalescing, semantic epoch diffs, directive policy, `Send + Sync` |
| `tests/retry.rs` | Integer backoff, cap/overflow safety, debounce reuse, non-retriable short-circuit, 408/429/5xx retry, network-change debounce, epoch reset, zero-attempt budget |
| `tests/event.rs` | 64-event capacity, overflow/resync, normalized monotonic snapshot refresh, snapshot acknowledgement, capacity-1 eviction, non-blocking producer after receiver drop |
| `tests/platform.rs` | 36-cell uniqueness and contract-only evidence, authentic Windows/macOS/Linux/Android/iOS API names, Web conditional/unsupported policy, mobile trust conditionality |
| `src/gpui_adapter.rs` (`#[gpui::test]`) | App global + `SystemServices`, missing-service error, generation-checked `NetworkSurfaceScope`, `NetworkEntityModel` lifecycle |
| `crates/gpui-box/tests/network.rs` | Facade re-exports, proxy/workload/trust use through `gpui_box::network`, generation-safe leases, `QueryClient` invalidation through both `drain_directives()` and `on_revalidation(...)` observer delivery, account teardown, `network-gpui` symbols/registration |

At this revision the default `gpui-network` suite runs **66** integration tests.
With `--all-features` it runs **71** (including five GPUI adapter tests), and the
`gpui-box` network feature suite runs **7** tests (one facade unit test and six
integration tests). These counts are descriptive; the commands above are
authoritative.

## Remaining acceptance work

- Windows/macOS/Linux native providers under #8: WinHTTP/NLM/CryptoAPI,
  Network.framework/CFNetwork/Security.framework and NetworkManager/portal
  adapters, with runtime evidence recorded per operation.
- Android provider under #2: `ConnectivityManager.NetworkCallback`, `ProxyInfo`,
  Network Security Config packaging and device evidence.
- iOS provider under #9: `NWPathMonitor`, `CFNetworkCopySystemProxySettings`,
  ATS/scene lifecycle and device evidence.
- Browser host under #18/#43: JavaScript `online`/`offline` bridge, browser-owned
  proxy/trust limits and secure-context qualification.
- Revalidation integration under #3/#54 for the actual HTTP pool owner; this
  crate emits scoped directives but does not mutate third-party connection pools.
- Real capability and secret-provider probes replacing the fake provider
  defaults; no row in this document claims a native provider or runtime
  qualification.

No row in this document claims `Evidence::Compiled` or
`Evidence::RuntimeVerified`. Contract compilation is not native qualification.
