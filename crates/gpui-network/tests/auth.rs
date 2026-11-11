mod support;

use gpui_network::*;
use gpui_secrets::{
    CredentialKey, CredentialProvider, DeleteReceipt, ProviderResult, SecretOperation, SecretPolicy,
};
use gpui_system::{Completion, ErrorCode};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use support::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AccessMode {
    Available,
    Locked,
    KeyInvalidated,
}

struct MemoryState {
    value: Option<Vec<u8>>,
    mode: AccessMode,
}

struct MemoryCredentials {
    state: Mutex<MemoryState>,
    calls: AtomicUsize,
}

impl MemoryCredentials {
    fn available(value: &[u8]) -> Self {
        Self {
            state: Mutex::new(MemoryState {
                value: Some(value.to_vec()),
                mode: AccessMode::Available,
            }),
            calls: AtomicUsize::new(0),
        }
    }

    fn empty() -> Self {
        Self {
            state: Mutex::new(MemoryState {
                value: None,
                mode: AccessMode::Available,
            }),
            calls: AtomicUsize::new(0),
        }
    }

    fn set_mode(&self, mode: AccessMode) {
        self.state.lock().unwrap().mode = mode;
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Acquire)
    }
}

impl CredentialProvider for MemoryCredentials {
    fn capabilities(&self) -> Vec<Capability<SecretOperation>> {
        [
            SecretOperation::Put,
            SecretOperation::Get,
            SecretOperation::Delete,
            SecretOperation::RemoveAccount,
        ]
        .into_iter()
        .map(|operation| Capability {
            operation,
            support: Support::Native,
            availability: Availability::Available,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: Vec::new(),
            observed_revision: 1,
        })
        .collect()
    }

    fn validate_policy(&self, _policy: SecretPolicy) -> Result<(), SystemError> {
        Ok(())
    }

    fn put(&self, _target: &str, value: &SecretBytes) -> ProviderResult<()> {
        self.state.lock().unwrap().value = Some(value.expose().to_vec());
        Ok(SecretAccess::Available(()))
    }

    fn get(&self, _target: &str) -> ProviderResult<Option<SecretBytes>> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let state = self.state.lock().unwrap();
        match state.mode {
            AccessMode::Locked => Ok(SecretAccess::Locked),
            AccessMode::KeyInvalidated => Ok(SecretAccess::KeyInvalidated),
            AccessMode::Available => match &state.value {
                Some(bytes) => Ok(SecretAccess::Available(Some(
                    SecretBytes::new(bytes.clone()).expect("bounded secret"),
                ))),
                None => Ok(SecretAccess::Available(None)),
            },
        }
    }

    fn delete(&self, _target: &str) -> ProviderResult<DeleteReceipt> {
        let previous = self.state.lock().unwrap().value.take();
        Ok(SecretAccess::Available(DeleteReceipt {
            existed: previous.is_some(),
        }))
    }

    fn list(&self, _prefix: &str) -> ProviderResult<Vec<String>> {
        Ok(SecretAccess::Available(Vec::new()))
    }
}

fn key() -> CredentialKey {
    CredentialKey::new("account@example.com", "network", "proxy").expect("key")
}

fn source(provider: &Arc<MemoryCredentials>) -> Arc<dyn ProxyCredentialSource> {
    let secrets = Secrets::with_provider("test.app", provider.clone()).expect("secrets");
    Arc::new(SecretsProxyCredentialSource::new(secrets, key()))
}

fn challenge(client: ClientId, proxy: &str, epoch: PolicyEpoch) -> ProxyAuthChallenge {
    ProxyAuthChallenge::new(client, origin(proxy), epoch, ProxyAuthScheme::Basic)
        .with_realm(Some("corporate proxy"))
        .expect("realm")
}

#[test]
fn proxy_headers_are_zeroized_redacted_and_only_exposed_explicitly() {
    let header = ProxyAuthHeader::basic(SecretBytes::new(b"canary".to_vec()).unwrap());
    assert_eq!(header.expose_header_value(), b"canary");
    assert_eq!(header.scheme(), ProxyAuthScheme::Basic);
    assert_eq!(header.header_name(), "Proxy-Authorization");
    let debug = format!("{header:?}");
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("canary"));

    let outcome = ProxyAuthOutcome::Header(header);
    assert!(outcome.is_authenticated());
    assert_eq!(outcome.kind(), ProxyAuthOutcomeKind::Header);
    let outcome_debug = format!("{outcome:?}");
    assert!(!outcome_debug.contains("canary"));
    assert!(outcome_debug.contains("Header"));
}

#[test]
fn proxy_auth_is_bounded_to_two_credential_bearing_attempts_per_epoch() {
    let (_, service) = fake_service();
    let provider = Arc::new(MemoryCredentials::available(b"Bearer canary"));
    service.set_proxy_credential_source(source(&provider));
    let client = ClientId(1);
    let epoch = service.policy_epoch();
    let request = challenge(client, "http://proxy.example:8080", epoch);

    let first = completed(service.handle_proxy_auth(&request, &context()));
    match first {
        ProxyAuthOutcome::Header(header) => {
            assert_eq!(header.expose_header_value(), b"Bearer canary")
        }
        other => panic!("unexpected first outcome: {other:?}"),
    }
    let second = completed(service.handle_proxy_auth(&request, &context()));
    assert!(second.is_authenticated());
    assert_eq!(provider.calls(), 2);

    let third = completed(service.handle_proxy_auth(&request, &context()));
    assert_eq!(third.kind(), ProxyAuthOutcomeKind::Exhausted);
    assert_eq!(
        provider.calls(),
        2,
        "exhausted attempt must not read secrets"
    );

    // A different proxy has its own bounded budget.
    let other_proxy = challenge(client, "http://proxy-two.example:8080", epoch);
    assert!(completed(service.handle_proxy_auth(&other_proxy, &context())).is_authenticated());
    assert_eq!(provider.calls(), 3);
}

#[test]
fn locked_invalidated_missing_and_explicit_sources_are_distinct() {
    let (_, service) = fake_service();
    let client = ClientId(2);
    let request = challenge(client, "http://proxy.example:8080", service.policy_epoch());

    let locked = Arc::new(MemoryCredentials::available(b"secret"));
    locked.set_mode(AccessMode::Locked);
    service.set_proxy_credential_source(source(&locked));
    let outcome = completed(service.handle_proxy_auth(&request, &context()));
    assert_eq!(outcome.kind(), ProxyAuthOutcomeKind::Locked);

    let invalidated = Arc::new(MemoryCredentials::available(b"secret"));
    invalidated.set_mode(AccessMode::KeyInvalidated);
    service.set_proxy_credential_source(source(&invalidated));
    let outcome = completed(service.handle_proxy_auth(&request, &context()));
    assert_eq!(outcome.kind(), ProxyAuthOutcomeKind::KeyInvalidated);

    let empty = Arc::new(MemoryCredentials::empty());
    service.set_proxy_credential_source(source(&empty));
    let outcome = completed(service.handle_proxy_auth(&request, &context()));
    assert_eq!(outcome.kind(), ProxyAuthOutcomeKind::MissingCredentials);

    service.clear_proxy_credential_source();
    let outcome = completed(service.handle_proxy_auth(&request, &context()));
    assert_eq!(outcome.kind(), ProxyAuthOutcomeKind::MissingCredentials);
}

#[test]
fn explicit_credential_source_bypasses_configured_state() {
    let (_, service) = fake_service();
    let provider = Arc::new(MemoryCredentials::available(b"token"));
    let secrets = Secrets::with_provider("test.app", provider.clone()).unwrap();
    let explicit = SecretsProxyCredentialSource::new(secrets, key());
    let request = challenge(
        ClientId(3),
        "http://proxy.example:8080",
        service.policy_epoch(),
    );
    let outcome = completed(service.handle_proxy_auth_with(&request, &explicit, &context()));
    assert!(outcome.is_authenticated());
    assert_eq!(provider.calls(), 1);
}

#[test]
fn proxy_credentials_require_a_background_context() {
    let (_, service) = fake_service();
    let provider = Arc::new(MemoryCredentials::available(b"secret"));
    service.set_proxy_credential_source(source(&provider));
    let owner = UiOwner::new(SurfaceId(5));
    let ui = owner.context(None).expect("ui context");
    let context = OperationContext {
        ui: Some(ui),
        ..OperationContext::default()
    };
    let request = challenge(
        ClientId(4),
        "http://proxy.example:8080",
        service.policy_epoch(),
    );
    let error = service
        .handle_proxy_auth(&request, &context)
        .expect_err("UI context must be rejected");
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(provider.calls(), 0);
}

#[test]
fn expired_and_cancelled_contexts_never_read_credentials() {
    let (_, service) = fake_service();
    let provider = Arc::new(MemoryCredentials::available(b"secret"));
    service.set_proxy_credential_source(source(&provider));
    let request = challenge(
        ClientId(5),
        "http://proxy.example:8080",
        service.policy_epoch(),
    );

    let context = OperationContext {
        deadline: Some(Instant::now()),
        ..OperationContext::default()
    };
    assert_eq!(
        service
            .handle_proxy_auth(&request, &context)
            .unwrap_err()
            .code,
        ErrorCode::TimedOut
    );
    assert_eq!(provider.calls(), 0);

    let cancellation = CancellationToken::default();
    cancellation.cancel(CancelReason::Caller);
    let context = OperationContext {
        cancellation,
        ..OperationContext::default()
    };
    assert!(matches!(
        service
            .handle_proxy_auth(&request, &context)
            .expect("cancelled is not an error"),
        Completion::Cancelled(CancelReason::Caller)
    ));
    assert_eq!(provider.calls(), 0);
}

#[test]
fn credential_rotation_resets_attenuation_and_is_scope_filtered() {
    let (_, service) = fake_service();
    let provider = Arc::new(MemoryCredentials::available(b"new-secret"));
    service.set_proxy_credential_source(source(&provider));
    let account = AccountScopeKey::new("first@example.com").unwrap();
    let other_account = AccountScopeKey::new("second@example.com").unwrap();
    let first = completed(service.register_client(ClientOwnerScope::account(account), &context()));
    let second =
        completed(service.register_client(ClientOwnerScope::account(other_account), &context()));

    let epoch = service.policy_epoch();
    let request = challenge(first.client_id(), "http://proxy.example:8080", epoch);
    assert!(completed(service.handle_proxy_auth(&request, &context())).is_authenticated());
    assert!(completed(service.handle_proxy_auth(&request, &context())).is_authenticated());
    assert_eq!(
        completed(service.handle_proxy_auth(&request, &context())).kind(),
        ProxyAuthOutcomeKind::Exhausted
    );

    let rotated = service.rotate_proxy_credentials(Some(&first.scope()));
    assert_eq!(rotated.directives.len(), 2);
    assert!(rotated.epoch > rotated.previous_epoch);
    assert!(
        first
            .drain_directives()
            .iter()
            .any(|directive| { directive.directive == RevalidationDirective::ReauthenticateProxy })
    );
    assert!(first.drain_directives().is_empty());
    assert!(second.drain_directives().is_empty());

    let request = challenge(
        first.client_id(),
        "http://proxy.example:8080",
        service.policy_epoch(),
    );
    assert!(completed(service.handle_proxy_auth(&request, &context())).is_authenticated());
    assert_eq!(provider.calls(), 3);
}

#[test]
fn realms_are_bounded_and_control_characters_are_rejected() {
    let base = |realm: Option<&str>| {
        ProxyAuthChallenge::new(
            ClientId(1),
            origin("http://proxy.example:8080"),
            PolicyEpoch::INITIAL,
            ProxyAuthScheme::Bearer,
        )
        .with_realm(realm)
    };
    assert_eq!(base(Some("  realm  ")).unwrap().realm(), Some("realm"));
    assert_eq!(base(Some("   ")).unwrap().realm(), None);
    assert_eq!(
        base(Some("bad\nrealm")).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        base(Some(&"r".repeat(129))).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn secrets_source_reads_only_through_the_explicit_key() {
    let provider = Arc::new(MemoryCredentials::available(b"stored"));
    let secrets = Secrets::with_provider("test.app", provider.clone()).unwrap();
    let source = SecretsProxyCredentialSource::new(secrets, key());
    let request = challenge(
        ClientId(1),
        "http://proxy.example:8080",
        PolicyEpoch::INITIAL,
    );
    match source.credential(&request, &context()).expect("get") {
        Completion::Completed(SecretAccess::Available(Some(bytes))) => {
            assert_eq!(bytes.expose(), b"stored")
        }
        _ => panic!("unexpected credential access"),
    }
    assert_eq!(provider.calls(), 1);
}

#[test]
fn dynamic_proxy_invalidation_resets_the_authentication_budget() {
    let (fake, service) = fake_service();
    let provider = Arc::new(MemoryCredentials::available(b"rotated-secret"));
    service.set_proxy_credential_source(source(&provider));
    let pac = ProxyConfig::pac_url(origin("https://pac.example"), "corp").expect("PAC config");

    for (index, config) in [ProxyConfig::SystemDynamic, pac].into_iter().enumerate() {
        service
            .set_proxy_config(config)
            .expect("dynamic proxy config");
        let epoch = service.policy_epoch();
        let request = challenge(
            ClientId(70 + index as u64),
            "http://proxy.example:8080",
            epoch,
        );
        assert!(completed(service.handle_proxy_auth(&request, &context())).is_authenticated());
        assert!(completed(service.handle_proxy_auth(&request, &context())).is_authenticated());
        let calls = provider.calls();
        assert_eq!(
            completed(service.handle_proxy_auth(&request, &context())).kind(),
            ProxyAuthOutcomeKind::Exhausted
        );
        assert_eq!(
            provider.calls(),
            calls,
            "the exhausted attempt must not read secrets"
        );

        if index == 0 {
            fake.invalidate_proxy_configuration();
        } else {
            assert!(service.invalidate_proxy_configuration().changed);
        }
        let refreshed_epoch = service.policy_epoch();
        assert_eq!(refreshed_epoch, epoch.next());

        let refreshed = challenge(
            ClientId(70 + index as u64),
            "http://proxy.example:8080",
            refreshed_epoch,
        );
        assert!(
            completed(service.handle_proxy_auth(&refreshed, &context())).is_authenticated(),
            "a dynamic invalidation must start a fresh authentication budget"
        );
        assert_eq!(provider.calls(), calls + 1);
    }
}
